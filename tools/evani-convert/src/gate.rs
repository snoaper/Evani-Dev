//! The quality gate (STAGE3_PLAN S3-6, `--gate`): every tier is rendered in
//! the engine's own renderer (apps/model-view's gate page, through
//! tools/convert-runner) from eight fixed views, original against
//! converted, and scored with SSIMULACRA2 and our worst-pixel measure
//! (metric.rs). The cheapest setting on each tier's quality ladder that
//! still passes on the worst of the eight views is the one the tier file is
//! made with; the real tier file is then rendered and scored again.
//!
//! The thresholds are provisional until Chris's blind test (decision 4):
//! `--gate-pairs <folder>` writes picture pairs for the calibration page
//! (`--calibration <folder>`), and `--calibration-result <folder>` suggests
//! thresholds from his answers.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::metric::{self, Rgba};
use crate::runner::{Job, Pose, Runner};
use crate::texture::ImageUse;
use crate::tools::{Tools, s};
use crate::{LADDER, TierLadder, glb};

/// SSIMULACRA2 at or above this on the worst view (provisional).
pub const SSIM_MIN: f64 = 90.0;
/// Worst-pixel (99.9th percentile) at or below this on the worst view (provisional).
pub const WORST_MAX: f64 = 0.35;
pub const THRESHOLDS_STATUS: &str = "provisional until Chris's blind test";
/// The format floor: each tier's best rung (unsimplified, 16-bit vertices,
/// full textures) is the most the shipped format can give, and its 16-bit
/// positions alone move outlines by a fraction of a pixel, which
/// SSIMULACRA2 scores in the 80s on a toon render (kart_pip: unquantised
/// 100, 16-bit 81). A tier's SSIMULACRA2 bar is therefore
/// `min(SSIM_MIN, floor - FLOOR_SLACK)` (provisional).
pub const FLOOR_SLACK: f64 = 5.0;
/// The floor counts only when the best rung itself looks right: SSIMULACRA2
/// at least this and the worst-pixel ceiling met. Otherwise the bar stays
/// SSIM_MIN and the tier fails as a broken conversion. Clean 16-bit rounding
/// measured 80.8 to 85 on kart_pip's worst view (provisional), so a floor
/// much below that is a fault, not rounding.
pub const FLOOR_MIN: f64 = 78.0;

/// Does the best rung's score count as the format floor?
pub fn floor_counts(floor_ssim: f64, floor_worst: f64) -> bool {
    floor_ssim >= FLOOR_MIN && floor_worst <= WORST_MAX
}

/// The camera's vertical field of view for the fitted views (the engine's default).
const FOV_DEG: f64 = 50.0;
/// The fitted views leave this share of the picture free on every side.
const MARGIN: f64 = 0.06;
/// The fitted eye is never nearer the centre of the normalised model
/// (bounding sphere 1) than this.
const NEAREST: f64 = 1.15;
/// Grid cells per side for the close-up's densest detail.
const CELLS: usize = 8;

/// Does a setting pass the absolute thresholds?
pub fn passes(ssim: f64, worst: f64) -> bool {
    passes_at(ssim, worst, SSIM_MIN)
}

/// Does a setting pass a tier's SSIMULACRA2 bar (see `bar_for`)?
pub fn passes_at(ssim: f64, worst: f64, bar: f64) -> bool {
    ssim >= bar && worst <= WORST_MAX
}

/// A tier's SSIMULACRA2 bar from its format floor (the best rung's worst
/// view): `min(SSIM_MIN, floor - FLOOR_SLACK)` when the best rung looks
/// right (SSIMULACRA2 at least FLOOR_MIN, worst pixel within WORST_MAX),
/// else SSIM_MIN.
pub fn bar_for(floor_ssim: f64, floor_worst: f64) -> f64 {
    if floor_counts(floor_ssim, floor_worst) { SSIM_MIN.min(floor_ssim - FLOOR_SLACK) } else { SSIM_MIN }
}

// ------------------------------------------------------------------ views

/// One fixed view: a pose and the part of the picture the model can cover
/// (fractions `[x0, y0, x1, y1]`), which is what gets scored.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub name: &'static str,
    pub pose: Pose,
    pub crop: [f64; 4],
}

/// The model normalised into a unit sphere at the origin (`place`), and
/// the eight views around it.
#[derive(Clone, Debug, PartialEq)]
pub struct Rig {
    /// `[x, y, z, scale]` for the model instance.
    pub place: [f64; 4],
    pub views: Vec<View>,
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt();
    if l > 1e-12 { [a[0] / l, a[1] / l, a[2] / l] } else { [0.0, 0.0, 1.0] }
}

/// A pose looking at `target` from `eye`, with a sane up vector.
fn look(eye: [f64; 3], target: [f64; 3], fov: f64) -> Pose {
    let f = norm(sub(target, eye));
    let up = if f[1].abs() > 0.95 { [0.0, 0.0, -f[1].signum()] } else { [0.0, 1.0, 0.0] };
    Pose { eye, target, up, fov_y_deg: fov }
}

/// Where the eight box corners land, as picture fractions `[x0, y0, x1, y1]`
/// (unclamped), or `None` when a corner is behind the near plane.
fn project_box(p: &Pose, lo: [f64; 3], hi: [f64; 3], w: u32, h: u32) -> Option<[f64; 4]> {
    let f = norm(sub(p.target, p.eye));
    let r = norm(cross(f, p.up));
    let u = cross(r, f);
    let t = (p.fov_y_deg.to_radians() / 2.0).tan();
    let aspect = f64::from(w.max(1)) / f64::from(h.max(1));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for c in 0..8 {
        let q = [if c & 1 == 0 { lo[0] } else { hi[0] }, if c & 2 == 0 { lo[1] } else { hi[1] }, if c & 4 == 0 { lo[2] } else { hi[2] }];
        let v = sub(q, p.eye);
        let z = dot(v, f);
        if z <= 0.1 {
            return None;
        }
        let nx = dot(v, r) / (z * t * aspect);
        let ny = dot(v, u) / (z * t);
        let (px, py) = ((nx + 1.0) / 2.0, (1.0 - ny) / 2.0);
        x0 = x0.min(px);
        x1 = x1.max(px);
        y0 = y0.min(py);
        y1 = y1.max(py);
    }
    Some([x0, y0, x1, y1])
}

/// Where the eight box corners land, as picture fractions with 4 pixels of
/// padding (the whole picture when a corner is behind the camera).
fn crop_for(p: &Pose, lo: [f64; 3], hi: [f64; 3], w: u32, h: u32) -> [f64; 4] {
    let Some([x0, y0, x1, y1]) = project_box(p, lo, hi, w, h) else { return [0.0, 0.0, 1.0, 1.0] };
    let (padx, pady) = (4.0 / f64::from(w.max(1)), 4.0 / f64::from(h.max(1)));
    [(x0 - padx).clamp(0.0, 1.0), (y0 - pady).clamp(0.0, 1.0), (x1 + padx).clamp(0.0, 1.0), (y1 + pady).clamp(0.0, 1.0)]
}

/// The eye on the ray from `target` along `dir` (unit) at the smallest
/// distance where the whole box `lo..hi` sits inside the picture with a
/// `MARGIN` border, so the model fills the frame (a sphere fit leaves a
/// flat model small). Never closer than `NEAREST` (outside the unit
/// sphere, room for the near plane). A fixed-count bisection: same input,
/// same eye.
fn fit_eye(dir: [f64; 3], target: [f64; 3], lo: [f64; 3], hi: [f64; 3], w: u32, h: u32) -> Pose {
    let at = |d: f64| look([target[0] + dir[0] * d, target[1] + dir[1] * d, target[2] + dir[2] * d], target, FOV_DEG);
    let fits = |d: f64| project_box(&at(d), lo, hi, w, h).is_some_and(|b| b[0] >= MARGIN && b[1] >= MARGIN && b[2] <= 1.0 - MARGIN && b[3] <= 1.0 - MARGIN);
    let (mut a, mut b) = (NEAREST, 64.0);
    if fits(a) {
        return at(a);
    }
    for _ in 0..48 {
        let m = (a + b) / 2.0;
        if fits(m) { b = m } else { a = m }
    }
    at(b)
}

/// The eight views for a model with box `lo..hi` (metres) and triangle
/// centroids `cents` (for the close-up), at a `w` x `h` window:
/// 4 around at 20 degrees up (azimuth 0 looks from +z), 2 high three-quarter
/// at 50 degrees up, 1 low front (eye at a tenth of the model's height),
/// 1 close-up on the densest 8x8x8 cell.
pub fn rig(lo: [f32; 3], hi: [f32; 3], cents: &[[f32; 3]], w: u32, h: u32) -> Rig {
    let lo = lo.map(f64::from);
    let hi = hi.map(f64::from);
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let radius = (dot(sub(hi, lo), sub(hi, lo)).sqrt() / 2.0).max(1e-6);
    let s = 1.0 / radius;
    let place = [-c[0] * s, -c[1] * s, -c[2] * s, s];
    // The normalised box.
    let nlo = [(lo[0] - c[0]) * s, (lo[1] - c[1]) * s, (lo[2] - c[2]) * s];
    let nhi = [(hi[0] - c[0]) * s, (hi[1] - c[1]) * s, (hi[2] - c[2]) * s];
    // Unit direction from the target to the eye.
    let orbit = |el: f64, az: f64| {
        let (el, az) = (el.to_radians(), az.to_radians());
        [el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]
    };
    let o = [0.0; 3];
    let around = |el: f64, az: f64| fit_eye(orbit(el, az), o, nlo, nhi, w, h);
    let mut v: Vec<(&'static str, Pose)> = vec![
        ("front", around(20.0, 0.0)),
        ("right", around(20.0, 90.0)),
        ("back", around(20.0, 180.0)),
        ("left", around(20.0, 270.0)),
        ("high front-right", around(50.0, 45.0)),
        ("high back-left", around(50.0, 225.0)),
    ];
    // Low front: the eye level with a tenth of the model's height, looking
    // level at the front (fitted along that level line).
    let low_y = nlo[1] + 0.1 * (nhi[1] - nlo[1]);
    v.push(("low front", fit_eye([0.0, 0.0, 1.0], [0.0, low_y, 0.0], nlo, nhi, w, h)));
    // Close-up: the cell with the most triangle centroids.
    let mut counts = vec![0u32; CELLS * CELLS * CELLS];
    let cell = |p: [f32; 3], i: usize| -> usize {
        let span = (hi[i] - lo[i]).max(1e-9);
        (((f64::from(p[i]) - lo[i]) / span * CELLS as f64) as isize).clamp(0, CELLS as isize - 1) as usize
    };
    for p in cents {
        counts[(cell(*p, 0) * CELLS + cell(*p, 1)) * CELLS + cell(*p, 2)] += 1;
    }
    let best = counts.iter().enumerate().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0))).filter(|m| *m.1 > 0).map(|m| m.0);
    let (cc, r_cell) = match best {
        Some(k) => {
            let (ix, iy, iz) = (k / (CELLS * CELLS), (k / CELLS) % CELLS, k % CELLS);
            let ext = sub(nhi, nlo);
            let at = |i: usize, n: usize| nlo[i] + (n as f64 + 0.5) * ext[i] / CELLS as f64;
            let half = [ext[0] / CELLS as f64 / 2.0, ext[1] / CELLS as f64 / 2.0, ext[2] / CELLS as f64 / 2.0];
            ([at(0, ix), at(1, iy), at(2, iz)], dot(half, half).sqrt())
        }
        None => ([0.0; 3], 0.5),
    };
    let dist = dot(cc, cc).sqrt();
    let dir = if dist > 1e-6 { norm(cc) } else { norm(orbit(20.0, 30.0)) };
    // Outside the unit sphere (so outside the box) with room for the 0.1 near plane.
    let t = (1.15 - dist).max(0.3);
    let eye = [cc[0] + dir[0] * t, cc[1] + dir[1] * t, cc[2] + dir[2] * t];
    let fov = (2.0 * (r_cell / t).min(1.0).asin().to_degrees() * 1.1).clamp(10.0, FOV_DEG);
    v.push(("close-up", look(eye, cc, fov)));
    let views = v.into_iter().map(|(name, pose)| View { name, pose, crop: crop_for(&pose, nlo, nhi, w, h) }).collect();
    Rig { place, views }
}

/// Every triangle's centroid in world space (empty when the geometry
/// cannot be read, e.g. meshopt-compressed buffers).
pub fn centroids(g: &evani_gltf::Gltf) -> Vec<[f32; 3]> {
    let world = g.world_matrices();
    let mut out = Vec::new();
    for (n, node) in g.nodes.iter().enumerate() {
        let Some(m) = node.mesh.and_then(|m| g.meshes.get(m)) else { continue };
        for p in &m.primitives {
            if p.mode != 4 {
                continue;
            }
            let Some(pos) = p.attribute("POSITION").and_then(|a| g.read_f32(a).ok()) else { continue };
            let pts: Vec<[f32; 3]> = pos.as_chunks::<3>().0.iter().map(|q| evani_gltf::transform_point(&world[n], *q)).collect();
            let idx: Vec<u32> = match p.indices {
                Some(i) => g.read_u32(i).unwrap_or_default(),
                None => (0..pts.len() as u32).collect(),
            };
            for t in idx.as_chunks::<3>().0 {
                let (Some(a), Some(b), Some(c)) = (pts.get(t[0] as usize), pts.get(t[1] as usize), pts.get(t[2] as usize)) else { continue };
                out.push([(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0]);
            }
        }
    }
    out
}

/// The tier's window: `target_resolution`, else the bench's 1920 x 1080.
pub fn window(root: &Path, tier: &str) -> Result<(u32, u32), String> {
    let p = root.join("assets").join("tiers").join(format!("{tier}.json"));
    let j: Value = serde_json::from_str(&std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))?;
    let r = j.get("target_resolution").and_then(Value::as_array);
    let dim = |i: usize| r.and_then(|a| a.get(i)).and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok());
    Ok(match (dim(0), dim(1)) {
        (Some(w), Some(h)) => (w, h),
        _ => (1920, 1080),
    })
}

// ----------------------------------------------------------------- search

/// Greedy binary search over knobs, in order: each knob's ladder runs from
/// the tier's own setting (index 0) to cheaper ones; for each knob the
/// highest index that still passes (with the knobs before it at their
/// chosen values) is kept. `None` when the starting setting fails.
/// Each setting is tried at most once (cached).
pub fn search(lens: &[usize], pass: &mut dyn FnMut(&[usize]) -> Result<bool, String>) -> Result<Option<Vec<usize>>, String> {
    let mut cache: BTreeMap<Vec<usize>, bool> = BTreeMap::new();
    let mut test = |idx: &[usize], pass: &mut dyn FnMut(&[usize]) -> Result<bool, String>| -> Result<bool, String> {
        if let Some(b) = cache.get(idx) {
            return Ok(*b);
        }
        let b = pass(idx)?;
        cache.insert(idx.to_vec(), b);
        Ok(b)
    };
    let mut cur = vec![0usize; lens.len()];
    if !test(&cur, pass)? {
        return Ok(None);
    }
    for k in 0..lens.len() {
        let (mut lo, mut hi) = (0usize, lens[k].saturating_sub(1));
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let mut t = cur.clone();
            t[k] = mid;
            if test(&t, pass)? {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        cur[k] = lo;
    }
    Ok(Some(cur))
}

/// One point on a tier's quality ladder.
#[derive(Clone, Debug, PartialEq)]
pub struct Setting {
    pub ratio: f64,
    pub error: f64,
    /// Texture size cap (none for a model without textures).
    pub texture: Option<u32>,
    pub vp: u32,
    pub vn: u32,
    pub vt: u32,
}

impl Setting {
    pub fn key(&self) -> String {
        format!("r{}_e{}_t{}_p{}_n{}_u{}", self.ratio, self.error, self.texture.unwrap_or(0), self.vp, self.vn, self.vt)
    }
    pub fn to_json(&self) -> Value {
        json!({ "ratio": self.ratio, "error": self.error, "texture_size": self.texture, "vp": self.vp, "vn": self.vn, "vt": self.vt })
    }
    pub fn describe(&self) -> String {
        let tex = self.texture.map_or(String::new(), |t| format!(", textures {t}"));
        format!("ratio {} (error {}){tex}, bits vp {} vn {} vt {}", self.ratio, self.error, self.vp, self.vn, self.vt)
    }
}

/// Each knob's ladder for one tier, cheapest last.
#[derive(Clone, Debug, PartialEq)]
pub struct Knobs {
    pub ratio: Vec<(f64, f64)>,
    pub texture: Vec<Option<u32>>,
    pub vp: Vec<u32>,
    pub vn: Vec<u32>,
    pub vt: Vec<u32>,
    /// The tier's own setting (ladder.json's tier entry): what its file keeps
    /// when no setting on the ladder passes.
    pub own: Setting,
}

impl Knobs {
    /// From ladder.json's `gate` key: every rung, the best first, with the
    /// tier's own setting (ladder.json's tier entry) slotted in where it
    /// belongs. The search backs off from the best toward the cheapest, so a
    /// tier whose own setting fails gets a better one rather than a fail.
    /// Textures start at the tier's screen-based size (more is never seen).
    /// `simplify` false (Evani.Model input): the geometry and textures stay;
    /// only the vertex bits are searched.
    pub fn for_tier(lad: &TierLadder, texture: Option<u32>, simplify: bool) -> Result<Knobs, String> {
        let j: Value = serde_json::from_str(LADDER).map_err(|e| format!("ladder.json: {e}"))?;
        let g = j.get("gate").ok_or("ladder.json has no gate key")?;
        let nums = |k: &str| -> Vec<u32> { g.get(k).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).filter_map(|v| u32::try_from(v).ok()).collect() };
        let full = |own: u32, k: &str| -> Vec<u32> {
            let mut v = nums(k);
            v.push(own);
            v.sort_unstable_by(|a, b| b.cmp(a));
            v.dedup();
            v
        };
        let r0 = lad.lods.first().map_or((1.0, 0.0), |r| (r.ratio, r.error));
        let mut ratio = vec![r0];
        let mut texture_l = vec![texture];
        if simplify {
            for p in g.get("ratio").and_then(Value::as_array).into_iter().flatten() {
                let (Some(r), Some(e)) = (p.get(0).and_then(Value::as_f64), p.get(1).and_then(Value::as_f64)) else { continue };
                if r != r0.0 {
                    ratio.push((r, e));
                }
            }
            ratio.sort_by(|a, b| b.0.total_cmp(&a.0));
            let min = g.get("texture_min").and_then(Value::as_u64).unwrap_or(64) as u32;
            if let Some(t) = texture {
                for div in nums("texture_div").into_iter().filter(|d| *d > 1) {
                    let sz = t / div;
                    if sz >= min {
                        texture_l.push(Some(sz));
                    }
                }
            }
        }
        let own = Setting { ratio: r0.0, error: r0.1, texture, vp: lad.vp, vn: lad.vn, vt: lad.vt };
        Ok(Knobs { ratio, texture: texture_l, vp: full(lad.vp, "vp"), vn: full(lad.vn, "vn"), vt: full(lad.vt, "vt"), own })
    }

    /// Knob order: ratio, texture, position bits, normal bits, UV bits.
    pub fn lens(&self) -> Vec<usize> {
        vec![self.ratio.len(), self.texture.len(), self.vp.len(), self.vn.len(), self.vt.len()]
    }

    pub fn at(&self, i: &[usize]) -> Setting {
        let (ratio, error) = self.ratio[i[0].min(self.ratio.len() - 1)];
        Setting { ratio, error, texture: self.texture[i[1].min(self.texture.len() - 1)], vp: self.vp[i[2].min(self.vp.len() - 1)], vn: self.vn[i[3].min(self.vn.len() - 1)], vt: self.vt[i[4].min(self.vt.len() - 1)] }
    }
}

// ----------------------------------------------------------------- scores

/// One view's scores.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewScore {
    pub ssim: f64,
    pub worst: f64,
    /// The worst pixel, in the whole picture.
    pub at: [usize; 2],
}

/// The pixel rectangle `[x, y, w, h]` of a crop on a `w` x `h` picture.
pub fn crop_px(c: [f64; 4], w: usize, h: usize) -> [usize; 4] {
    let x0 = (c[0] * w as f64).floor().max(0.0) as usize;
    let y0 = (c[1] * h as f64).floor().max(0.0) as usize;
    let x1 = ((c[2] * w as f64).ceil() as usize).min(w);
    let y1 = ((c[3] * h as f64).ceil() as usize).min(h);
    // At least 8 x 8 (SSIMULACRA2's smallest), inside the picture.
    let (x0, x1) = if x1 < x0 + 8 { (x0.min(w.saturating_sub(8)), (x0.min(w.saturating_sub(8)) + 8).min(w)) } else { (x0, x1) };
    let (y0, y1) = if y1 < y0 + 8 { (y0.min(h.saturating_sub(8)), (y0.min(h.saturating_sub(8)) + 8).min(h)) } else { (y0, y1) };
    [x0, y0, x1 - x0, y1 - y0]
}

/// Score every view of `got` against `refs` inside each view's crop.
pub fn score_views(rig: &Rig, refs: &[Rgba], got: &[Rgba]) -> Result<Vec<ViewScore>, String> {
    if refs.len() != rig.views.len() || got.len() != rig.views.len() {
        return Err(format!("{} views, {} original and {} converted pictures", rig.views.len(), refs.len(), got.len()));
    }
    let mut out = Vec::new();
    for ((v, a), b) in rig.views.iter().zip(refs).zip(got) {
        let r = crop_px(v.crop, a.w, a.h);
        let (sc, _) = metric::score(&a.crop(r), &b.crop(r))?;
        out.push(ViewScore { ssim: sc.ssimulacra2, worst: sc.worst_pixel, at: [r[0] + sc.worst_at[0], r[1] + sc.worst_at[1]] });
    }
    Ok(out)
}

/// The worst view: (lowest SSIMULACRA2, highest worst pixel, index of the view that sets the verdict).
pub fn worst_of(v: &[ViewScore]) -> (f64, f64, usize) {
    let ssim = v.iter().map(|x| x.ssim).fold(f64::MAX, f64::min);
    let worst = v.iter().map(|x| x.worst).fold(0.0, f64::max);
    // The view furthest from passing: the lower SSIMULACRA2 margin, or the higher worst-pixel overshoot.
    let k = v
        .iter()
        .enumerate()
        .max_by(|a, b| {
            let m = |x: &ViewScore| ((SSIM_MIN - x.ssim) / 10.0).max((x.worst - WORST_MAX) / 0.1);
            m(a.1).total_cmp(&m(b.1))
        })
        .map_or(0, |m| m.0);
    (if v.is_empty() { 0.0 } else { ssim }, worst, k)
}

/// How far (SSIMULACRA2) a finished tier file may score under the gate's
/// trial of the same setting. Renders are deterministic and the candidate is
/// made as the tier file is, textures included: a textured trial is scored
/// on every device texture family ([`FAMILIES`]) from the same encoder
/// the tier file's sets come from (S7-5), so its scores match the file's.
pub const DRIFT_MAX: f64 = 3.0;

/// Does a finished file's score `fin` drift from its trial's `trial` (the
/// same texture family for a textured file)? Only a drop counts: the
/// finished file is judged against the bar on its own score as well, so one
/// that scores better than its trial can't let a worse file through;
/// `geometry_difference` checks the geometry too.
pub fn drifted(trial: f64, fin: f64) -> bool {
    trial - fin > DRIFT_MAX
}

/// The texture families a device samples a tier file's maps in, the
/// engine's `choose_format` outcomes for the sets the converter writes:
/// ASTC 4x4 (phones), BC7 (desktops) and the PNG/JPEG core image (every
/// other device). A textured trial and its finished file are scored on all
/// three and judged on the worst, since any tier can meet any device.
pub use evani_guide::tier_fetch::FAMILIES;

/// One texture family's scores (`family` is `None` for an untextured file).
#[derive(Clone, Debug, PartialEq)]
pub struct FamilyViews {
    pub family: Option<&'static str>,
    pub views: Vec<ViewScore>,
}

/// The family that scores worst (lowest worst-view SSIMULACRA2, the first
/// on a tie), or `None` for an empty list.
pub fn worst_family(f: &[FamilyViews]) -> Option<usize> {
    f.iter().enumerate().min_by(|a, b| worst_of(&a.1.views).0.total_cmp(&worst_of(&b.1.views).0)).map(|(k, _)| k)
}

/// Every family passes the bar.
pub fn families_pass(f: &[FamilyViews], bar: f64) -> bool {
    !f.is_empty() && f.iter().all(|x| {
        let (ssim, worst, _) = worst_of(&x.views);
        passes_at(ssim, worst, bar)
    })
}

/// The largest drop from a trial's family scores to the finished file's, over
/// the families both were scored on.
pub fn family_drift(trial: &[FamilyViews], fin: &[FamilyViews]) -> Option<(f64, Option<&'static str>, f64, f64)> {
    fin.iter()
        .filter_map(|f| trial.iter().find(|t| t.family == f.family).map(|t| (worst_of(&t.views).0 - worst_of(&f.views).0, f.family, worst_of(&t.views).0, worst_of(&f.views).0)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
}

/// What the renderer draws from a file's geometry, one line per drawn
/// primitive (its node, draw mode, index and vertex counts and attribute
/// formats) plus the `extras.evani.lods` list. Images, materials and other
/// metadata are left out.
pub fn geometry_signature(j: &Value) -> Vec<String> {
    let acc = |i: Option<u64>| -> String {
        let Some(a) = i.and_then(|i| j.pointer(&format!("/accessors/{i}"))) else { return "-".into() };
        let norm = if a.get("normalized").and_then(Value::as_bool) == Some(true) { " normalized" } else { "" };
        format!("{} {} of {}{norm}", a["count"], a["type"].as_str().unwrap_or("?"), a["componentType"])
    };
    let mut out = Vec::new();
    for (k, n) in j.get("nodes").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let Some(m) = n.get("mesh").and_then(Value::as_u64) else { continue };
        let name = n.get("name").and_then(Value::as_str).unwrap_or("");
        for (p, prim) in j.pointer(&format!("/meshes/{m}/primitives")).and_then(Value::as_array).into_iter().flatten().enumerate() {
            let mut attrs: Vec<String> = prim.get("attributes").and_then(Value::as_object).into_iter().flatten().map(|(a, i)| format!("{a} {}", acc(i.as_u64()))).collect();
            attrs.sort();
            let mode = prim.get("mode").and_then(Value::as_u64).unwrap_or(4);
            out.push(format!("node {k} '{name}' primitive {p}: mode {mode}, indices {}, {}", acc(prim.get("indices").and_then(Value::as_u64)), attrs.join(", ")));
        }
    }
    out.push(format!("lods {}", j.pointer("/extras/evani/lods").map_or_else(|| "none".to_owned(), Value::to_string)));
    out
}

/// The first way a finished tier file's geometry differs from the gate's
/// candidate of the same setting, or `None` when they match.
pub fn geometry_difference(candidate: &Value, finished: &Value) -> Option<String> {
    let (a, b) = (geometry_signature(candidate), geometry_signature(finished));
    if a == b {
        return None;
    }
    let k = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let line = |v: &[String]| v.get(k).cloned().unwrap_or_else(|| "nothing".into());
    Some(format!("{} drawn primitives against {}; first difference: trial has {}, file has {}", a.len() - 1, b.len() - 1, line(&a), line(&b)))
}

/// A pixel counts as the model when a channel differs from its row's
/// backdrop by more than this (0 to 255).
pub const COVER_DIFF: u8 = 12;
/// Below this share of covered pixels in every view, the model drew nothing.
pub const COVER_MIN: f64 = 0.005;

/// The share of `img` the model covers: pixels that differ by more than
/// [`COVER_DIFF`] in a channel from their row's backdrop, taken as the
/// straight blend between the row's two end pixels (the views keep the model
/// off the picture's edges, so both ends are backdrop). A backdrop that
/// changes top to bottom or in a straight ramp left to right reads as
/// empty; a curved or patterned one would count as model.
pub fn coverage(img: &Rgba) -> f64 {
    if img.w == 0 || img.h == 0 {
        return 0.0;
    }
    let span = (img.w.max(2) - 1) as f64;
    let mut n = 0usize;
    for row in img.px.chunks_exact(img.w * 4) {
        let (first, last) = (&row[..4], &row[row.len() - 4..]);
        n += row
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(x, p)| {
                let t = *x as f64 / span;
                (0..3).any(|c| (p[c] as f64 - (first[c] as f64 + (last[c] as f64 - first[c] as f64) * t)).abs() > COVER_DIFF as f64)
            })
            .count();
    }
    n as f64 / (img.w * img.h) as f64
}

/// `Err` when the reference render shows nothing in any view: blank against
/// blank would score a false 100 on every setting.
pub fn check_drawn(refs: &[Rgba], what: &str) -> Result<(), String> {
    let best = refs.iter().map(coverage).fold(0.0, f64::max);
    if best < COVER_MIN {
        return Err(format!(
            "quality gate: {what} drew nothing (the model covers {:.2}% of its best view, under {:.1}%), so its scores would be a false 100; check that the renderer draws this model (skinned parts wait for `anim`, S3-8)",
            best * 100.0,
            COVER_MIN * 100.0
        ));
    }
    Ok(())
}

/// (mesh nodes, skinned mesh nodes) in a glTF's JSON. The engine's model
/// path skips skinned nodes until `anim` lands (S3-8), so the gate cannot
/// see them.
pub fn skinned_parts(j: &Value) -> (usize, usize) {
    let nodes = j.get("nodes").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let meshes: Vec<&Value> = nodes.iter().filter(|n| n.get("mesh").is_some()).collect();
    let skinned = meshes.iter().filter(|n| n.get("skin").is_some()).count();
    (meshes.len(), skinned)
}

/// What the gate does with a model, from its skinned parts.
#[derive(Clone, Debug, PartialEq)]
pub enum GatePlan {
    /// Not gated (not a failure): the note says why.
    Skip(String),
    /// Gated, with a note when part of the model is not seen.
    Run(Option<String>),
}

/// The plan for a model of `parts` (from [`skinned_parts`]).
pub fn skinned_plan(parts: (usize, usize)) -> GatePlan {
    match parts {
        (m, k) if m > 0 && k == m => GatePlan::Skip("not gated: skinned model, which the engine's renderer draws once `anim` lands (STAGE3_PLAN S3-8); the tier files keep the tier's own settings".into()),
        (m, k) if k > 0 => GatePlan::Run(Some(format!("quality gate: {k} of {m} mesh node(s) are skinned and not drawn until `anim` lands (S3-8), so the gate does not see them"))),
        _ => GatePlan::Run(None),
    }
}

/// Start the gate unless the master is skinned all through (then a note
/// says why it is not gated); a partly skinned master gets a note too.
#[allow(clippy::too_many_arguments)]
pub fn start_checked(root: &Path, out: &Path, work: &Path, id: &str, master: &Path, tools: &Tools, pairs: Option<PathBuf>, notes: &mut Vec<String>) -> Result<Option<Gate>, String> {
    let j = glb::Doc::read_file(master)?.json;
    match skinned_plan(skinned_parts(&j)) {
        GatePlan::Skip(note) => {
            eprintln!("  gate: {note}");
            notes.push(note);
            Ok(None)
        }
        GatePlan::Run(note) => {
            notes.extend(note);
            Gate::start(root, out, work, id, master, tools, pairs).map(Some)
        }
    }
}

fn views_json(rig: &Rig, v: &[ViewScore]) -> Vec<Value> {
    rig.views.iter().zip(v).map(|(w, x)| json!({ "view": w.name, "ssimulacra2": round(x.ssim, 2), "worst_pixel": round(x.worst, 4), "worst_at": x.at })).collect()
}

fn round(x: f64, d: i32) -> f64 {
    let p = 10f64.powi(d);
    (x * p).round() / p
}

// ------------------------------------------------------------- pictures

/// A compressed RGB PNG (zlib through flate2; CRC from evani-model).
pub fn png_rgb(w: usize, h: usize, rgb: &[u8]) -> Result<Vec<u8>, String> {
    png_8bit(w, h, 3, rgb)
}

/// A compressed RGBA PNG (the gate's decoded device textures, S7-5).
pub fn png_rgba(w: usize, h: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    png_8bit(w, h, 4, rgba)
}

/// An 8-bit PNG of `ch` channels (3: RGB, 4: RGBA).
fn png_8bit(w: usize, h: usize, ch: usize, rgb: &[u8]) -> Result<Vec<u8>, String> {
    if rgb.len() < w * h * ch {
        return Err(format!("png: {} bytes for {w}x{h}x{ch}", rgb.len()));
    }
    let mut raw = Vec::with_capacity((w * ch + 1) * h);
    for y in 0..h {
        raw.push(1u8); // filter: Sub
        let row = &rgb[y * w * ch..(y + 1) * w * ch];
        for x in 0..w * ch {
            let left = if x >= ch { row[x - ch] } else { 0 };
            raw.push(row[x].wrapping_sub(left));
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    z.write_all(&raw).map_err(|e| e.to_string())?;
    let idat = z.finish().map_err(|e| e.to_string())?;
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&evani_model::png::crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, if ch == 4 { 6 } else { 2 }, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &idat);
    chunk(b"IEND", &[]);
    Ok(out)
}

fn rgb_of(img: &Rgba) -> Vec<u8> {
    img.px.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect()
}

/// A plain RGB canvas.
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize, c: [u8; 3]) -> Canvas {
        Canvas { w, h, px: (0..w * h).flat_map(|_| c).collect() }
    }
    fn put(&mut self, x: usize, y: usize, c: [u8; 3]) {
        if x < self.w && y < self.h {
            let i = (y * self.w + x) * 3;
            self.px[i..i + 3].copy_from_slice(&c);
        }
    }
}

/// The difference map as a heat colour (black, red, yellow, white).
fn heat(v: f32) -> [u8; 3] {
    let v = v.clamp(0.0, 1.0) * 3.0;
    [(v.min(1.0) * 255.0) as u8, ((v - 1.0).clamp(0.0, 1.0) * 255.0) as u8, ((v - 2.0).clamp(0.0, 1.0) * 255.0) as u8]
}

/// Draw `img` (box-averaged) or `map` (max-pooled) to fit `bw` x `bh` at (`ox`, `oy`).
fn fit_into(cv: &mut Canvas, ox: usize, oy: usize, bw: usize, bh: usize, img: Option<&Rgba>, map: Option<(&[f32], usize, usize)>) {
    let (sw, sh) = img.map_or_else(|| map.map_or((1, 1), |m| (m.1, m.2)), |i| (i.w, i.h));
    if sw == 0 || sh == 0 {
        return;
    }
    let k = (bw as f64 / sw as f64).min(bh as f64 / sh as f64);
    let (dw, dh) = (((sw as f64 * k) as usize).max(1), ((sh as f64 * k) as usize).max(1));
    let (dx, dy) = (ox + (bw - dw.min(bw)) / 2, oy + (bh - dh.min(bh)) / 2);
    for y in 0..dh {
        let (y0, y1) = (y * sh / dh, ((y + 1) * sh / dh).max(y * sh / dh + 1).min(sh));
        for x in 0..dw {
            let (x0, x1) = (x * sw / dw, ((x + 1) * sw / dw).max(x * sw / dw + 1).min(sw));
            let c = if let Some(i) = img {
                let mut acc = [0u32; 3];
                let mut n = 0u32;
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        let p = &i.px[(yy * i.w + xx) * 4..];
                        acc[0] += u32::from(p[0]);
                        acc[1] += u32::from(p[1]);
                        acc[2] += u32::from(p[2]);
                        n += 1;
                    }
                }
                let n = n.max(1);
                [(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8]
            } else if let Some((m, mw, _)) = map {
                let mut v = 0f32;
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        v = v.max(m[yy * mw + xx]);
                    }
                }
                heat(v)
            } else {
                [0, 0, 0]
            };
            cv.put(dx + x, dy + y, c);
        }
    }
}

/// A `cw` x `ch` window at 1:1 centred on `at`, inside a `w` x `h` picture.
fn window_at(at: [usize; 2], w: usize, h: usize, cw: usize, ch: usize) -> [usize; 4] {
    let (cw, ch) = (cw.min(w), ch.min(h));
    let x = at[0].saturating_sub(cw / 2).min(w - cw);
    let y = at[1].saturating_sub(ch / 2).min(h - ch);
    [x, y, cw, ch]
}

const PANEL_W: usize = 384;
const PANEL_H: usize = 216;
const ZOOM: usize = 216;
const GAP: usize = 4;

/// The sheet: one row per view, original | converted | difference (each
/// fitted into 384 x 216, the difference max-pooled so no bad pixel is
/// lost), then 216 x 216 crops at 1:1 around the worst pixel: original,
/// converted, difference.
pub fn sheet(rig: &Rig, refs: &[Rgba], got: &[Rgba], scores: &[ViewScore]) -> Result<Vec<u8>, String> {
    let n = rig.views.len();
    let w = 3 * PANEL_W + 3 * ZOOM + 7 * GAP;
    let h = n * PANEL_H + (n + 1) * GAP;
    let mut cv = Canvas::new(w, h, [24, 24, 28]);
    for (i, ((a, b), sc)) in refs.iter().zip(got).zip(scores).enumerate() {
        let map = metric::error_map(a, b)?;
        let y = GAP + i * (PANEL_H + GAP);
        let mut x = GAP;
        fit_into(&mut cv, x, y, PANEL_W, PANEL_H, Some(a), None);
        x += PANEL_W + GAP;
        fit_into(&mut cv, x, y, PANEL_W, PANEL_H, Some(b), None);
        x += PANEL_W + GAP;
        fit_into(&mut cv, x, y, PANEL_W, PANEL_H, None, Some((&map, a.w, a.h)));
        x += PANEL_W + GAP;
        let r = window_at(sc.at, a.w, a.h, ZOOM, ZOOM);
        let zm: Vec<f32> = (r[1]..r[1] + r[3]).flat_map(|yy| map[yy * a.w + r[0]..yy * a.w + r[0] + r[2]].to_vec()).collect();
        fit_into(&mut cv, x, y, ZOOM, ZOOM, Some(&a.crop(r)), None);
        x += ZOOM + GAP;
        fit_into(&mut cv, x, y, ZOOM, ZOOM, Some(&b.crop(r)), None);
        x += ZOOM + GAP;
        fit_into(&mut cv, x, y, ZOOM, ZOOM, None, Some((&zm, r[2], r[3])));
    }
    png_rgb(cv.w, cv.h, &cv.px)
}

// ---------------------------------------------------------------- the gate

/// What a tier's candidates are made from.
pub enum Source<'a> {
    /// The general path: `build(tier, setting, file)` writes the tier's
    /// geometry for a setting exactly as the tier file gets it (its LOD
    /// chain, quantized and compressed, with `extras.evani.lods`); the gate
    /// then points its images at textures resized for the setting.
    General { build: &'a dyn Fn(&str, &Setting, &Path) -> Result<(), String>, uses: &'a [(ImageUse, PathBuf)] },
    /// An Evani.Model tier file: only re-quantised.
    EvaniModel { input: &'a Path },
}

/// One tried setting.
#[derive(Clone, Debug)]
struct Trial {
    setting: Setting,
    /// Every texture family scored (one `None` entry for an untextured
    /// file; a failed search trial stops at its first failing family).
    families: Vec<FamilyViews>,
    /// The worst family's views.
    views: Vec<ViewScore>,
    pass: bool,
    /// For the calibration pairs: the worst view's crop of the converted picture.
    pair: Option<PairCrop>,
}

/// A render file for one texture family (`None`: untextured).
type FamilyFile = (Option<&'static str>, PathBuf);

/// A view index, its crop rectangle and the converted picture's crop.
type PairCrop = (usize, [usize; 4], Rgba);

struct TierGate {
    tier: String,
    window: (u32, u32),
    render: (usize, usize),
    reference: String,
    refs: Vec<Rgba>,
    start: Setting,
    /// The tier's own setting, which its file keeps when nothing passes.
    own: Setting,
    /// The best rung's worst-view SSIMULACRA2 (the format floor) and the bar it sets.
    floor: f64,
    floor_used: bool,
    bar: f64,
    chosen: Option<Setting>,
    trials: Vec<Trial>,
    verified: Option<Trial>,
    /// The chosen setting's trial score less the finished file's.
    drift: Option<f64>,
    /// Whether the finished file's geometry is the chosen candidate's (`None`: no candidate to compare).
    geometry_same: Option<bool>,
    sheet: Option<String>,
    problems: Vec<String>,
}

/// The gate for one model: one runner (one Chrome) for every tier.
pub struct Gate {
    root: PathBuf,
    /// Served under `/m/` (the output folder).
    out: PathBuf,
    /// Candidates and pictures (`<out>/.work/gate`).
    dir: PathBuf,
    id: String,
    images_mjs: PathBuf,
    tools: Tools,
    pairs: Option<PathBuf>,
    rig_src: ([f32; 3], [f32; 3], Vec<[f32; 3]>),
    runner: Runner,
    /// A general model's reference render file and how the report names it:
    /// the master, or the master with the tangents the tier files get (see
    /// [`reference_for`]).
    reference: (PathBuf, String),
    tiers: Vec<TierGate>,
    renders: usize,
    started: std::time::Instant,
}

/// What a general model's tier files are scored against. Normally the
/// master. But the engine draws a normal map only with tangents
/// (evani-render `Part::tangents`), and the converter's clean gives the tier
/// files MikkTSpace tangents when a normal-mapped primitive has none
/// (`clean_file`): scored against the bare master, the tier file's normal
/// map would count as damage (the S3-12 look fixture: SSIMULACRA2 -71.6 at
/// the best rung, with or without `--look`). So then the reference is the
/// master given the same steps the clean takes before its tangents (the
/// engine's normals for a mesh without any, weld, glTF-Transform `tangents`),
/// written to the gate folder; the master file itself stays untouched.
fn reference_for(master: &Path, dir: &Path, tools: &Tools) -> Result<(PathBuf, String), String> {
    if !crate::needs_tangents(&glb::Doc::read_file(master)?.json) {
        return Ok((master.to_path_buf(), "master".to_owned()));
    }
    let normals = dir.join("reference.normals.glb");
    let welded = dir.join("reference.weld.glb");
    let reference = dir.join("reference.glb");
    let base = match crate::normals::add_engine_normals(master, &normals)? {
        0 => master.to_path_buf(),
        _ => normals,
    };
    tools.gltf_transform(&["weld", &s(&base), &s(&welded)], "gltf-transform weld (gate reference)")?;
    tools.gltf_transform(&["tangents", &s(&welded), &s(&reference)], "gltf-transform tangents (gate reference)")?;
    Ok((reference, "master, with the MikkTSpace tangents the tier files get for its normal maps".to_owned()))
}

fn rel(base: &Path, p: &Path) -> String {
    p.strip_prefix(base).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

impl Gate {
    /// Build the gate page if needed and start the runner. `master` is the
    /// model the views are fitted to.
    pub fn start(root: &Path, out: &Path, work: &Path, id: &str, master: &Path, tools: &Tools, pairs: Option<PathBuf>) -> Result<Gate, String> {
        let dist = crate::runner::gate_dist(root)?;
        let dir = work.join("gate");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let g = evani_gltf::Gltf::from_glb(&std::fs::read(master).map_err(|e| format!("{}: {e}", master.display()))?).map_err(|e| format!("{}: {}", master.display(), e.0))?;
        let (lo, hi) = crate::report::model_box(&g);
        let cents = centroids(&g);
        let reference = reference_for(master, &dir, tools)?;
        let runner = Runner::start(root, &tools.node, &dist, out)?;
        Ok(Gate {
            root: root.to_path_buf(),
            out: out.to_path_buf(),
            dir,
            id: id.to_owned(),
            images_mjs: work.join("images.mjs"),
            tools: tools.clone(),
            pairs,
            rig_src: (lo, hi, cents),
            runner,
            reference,
            tiers: Vec::new(),
            renders: 0,
            started: std::time::Instant::now(),
        })
    }

    fn rig(&self, w: u32, h: u32) -> Rig {
        rig(self.rig_src.0, self.rig_src.1, &self.rig_src.2, w, h)
    }

    fn render(&mut self, tier: &str, model: &Path, name: &str) -> Result<Vec<Rgba>, String> {
        let (w, h) = window(&self.root, tier)?;
        let rig = self.rig(w, h);
        let job = Job { tier: tier.to_owned(), width: w, height: h, model: rel(&self.out, model), place: rig.place, views: rig.views.iter().map(|v| v.pose).collect(), out: self.dir.join("shots").join(name) };
        self.renders += 1;
        self.runner.render(&job)
    }

    /// Make one candidate for `st`: the geometry file `{tier}-{key}.glb`
    /// (as the tier file gets it) and, for a textured setting, one render
    /// file per device texture family, `{tier}-{key}.{family}.glb`, whose
    /// images are what that family's devices sample (S7-5). An untextured
    /// candidate is the geometry file itself, family `None`.
    fn candidate(&self, tier: &str, src: &Source, st: &Setting) -> Result<(PathBuf, Vec<FamilyFile>), String> {
        let p = self.dir.join(format!("{tier}-{}.glb", st.key()));
        match src {
            Source::General { build, .. } => build(tier, st, &p)?,
            Source::EvaniModel { input } => {
                // As the tier file is packed (lib.rs `convert_evani_model`).
                let mut args: Vec<String> = vec!["-i".into(), s(input)];
                args.extend(["-kn", "-ke", "-kv"].map(String::from));
                args.extend(["-vp".into(), st.vp.to_string(), "-vt".into(), st.vt.to_string(), "-vn".into(), st.vn.to_string()]);
                args.extend(["-o".into(), s(&p), "-cc".into(), "-ce".into(), "ext".into()]);
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                self.tools.gltfpack(&a, &format!("gltfpack gate candidate ({tier}, {})", st.describe()))?;
            }
        }
        let (Source::General { uses, .. }, Some(size)) = (src, st.texture) else { return Ok((p.clone(), vec![(None, p)])) };
        if uses.is_empty() {
            return Ok((p.clone(), vec![(None, p)]));
        }
        let mut out = Vec::new();
        for family in FAMILIES {
            let names = self.device_textures(uses, size, family)?;
            let mut d = glb::Doc::read_file(&p)?;
            if let Some(Value::Array(images)) = d.json.get_mut("images") {
                for im in images.iter_mut() {
                    let uri = im.get("uri").and_then(Value::as_str).unwrap_or("").to_owned();
                    let file = glb::uri_path(&uri).replace('\\', "/");
                    let file = file.rsplit('/').next().unwrap_or("").to_owned();
                    let Some((u, name)) = uses.iter().zip(&names).find(|((_, src), _)| src.file_name().and_then(|n| n.to_str()) == Some(file.as_str())).map(|((u, _), c)| (u, c)) else {
                        return Err(format!("gate candidate: packed image {uri} is not one of the model's images"));
                    };
                    let mime = if family != "rgba" || u.core_ext() == "png" { "image/png" } else { "image/jpeg" };
                    *im = json!({ "name": u.name, "uri": name, "mimeType": mime });
                }
            }
            if let Some(Value::Object(o)) = d.json.pointer_mut("/extras/evani") {
                o.remove("textures");
            }
            let fp = self.dir.join(format!("{tier}-{}.{family}.glb", st.key()));
            std::fs::write(&fp, d.to_glb()?).map_err(|e| format!("{}: {e}", fp.display()))?;
            out.push((Some(family), fp));
        }
        Ok((p, out))
    }

    /// The model's images resized to `size` (once per size), as URIs relative
    /// to the gate folder: the core images (PNG or JPEG, as the tier file's)
    /// plus, for a JPEG core, the lossless PNG the device sets are encoded
    /// from (`{name}.src.png`), as lib.rs `encode_images` makes them.
    fn textures(&self, uses: &[(ImageUse, PathBuf)], size: u32) -> Result<Vec<String>, String> {
        let sub = format!("tex{size}");
        let dir = self.dir.join(&sub);
        let names: Vec<String> = uses.iter().map(|(u, _)| format!("{sub}/{}.{}", u.name, u.core_ext())).collect();
        let pngs: Vec<String> = uses.iter().zip(&names).map(|((u, _), n)| if u.core_ext() == "png" { n.clone() } else { format!("{sub}/{}.src.png", u.name) }).collect();
        if names.iter().chain(&pngs).all(|n| self.dir.join(n).is_file()) {
            return Ok(names);
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let jobs: Vec<Value> = uses.iter().zip(names.iter().zip(&pngs)).map(|((u, src), (n, png))| json!({ "src": s(src), "max": size, "core": s(&self.dir.join(n)), "core_format": u.core_ext(), "png": s(&self.dir.join(png)) })).collect();
        let jf = dir.join("images.json");
        std::fs::write(&jf, Value::Array(jobs).to_string()).map_err(|e| format!("{}: {e}", jf.display()))?;
        self.tools.node_script(&self.images_mjs, &[&s(&jf)], &format!("resize textures for the gate ({size})"))?;
        Ok(names)
    }

    /// What `family`'s devices sample at `size`, as URIs relative to the gate
    /// folder: `rgba` is the core images; `astc` and `bc7` encode the
    /// lossless resized PNG with the tier file's own encoder (lib.rs
    /// `encode_ktx`, the same arguments) and decode level 0 back to a PNG
    /// (`{name}.{family}.png`), once per size.
    fn device_textures(&self, uses: &[(ImageUse, PathBuf)], size: u32, family: &str) -> Result<Vec<String>, String> {
        let cores = self.textures(uses, size)?;
        if family == "rgba" {
            return Ok(cores);
        }
        let sub = format!("tex{size}");
        let mut out = Vec::new();
        for ((u, _), core) in uses.iter().zip(&cores) {
            let name = format!("{sub}/{}.{family}.png", u.name);
            let png = self.dir.join(&name);
            if !png.is_file() {
                let src = if u.core_ext() == "png" { self.dir.join(core) } else { self.dir.join(format!("{sub}/{}.src.png", u.name)) };
                let ktx = self.dir.join(format!("{sub}/{}.{family}.ktx2", u.name));
                crate::encode_ktx(&self.tools, family, u.role.srgb(), &src, &ktx)?;
                decode_set(&self.tools, family, &ktx, &png)?;
            }
            out.push(name);
        }
        Ok(out)
    }

    /// Search the tier's ladder. Returns the chosen setting (`None`: the
    /// tier's own setting already fails; the tier file keeps it).
    pub fn tune(&mut self, tier: &str, src: &Source, knobs: &Knobs) -> Result<Option<Setting>, String> {
        let (w, h) = window(&self.root, tier)?;
        let rig = self.rig(w, h);
        let (reference_path, reference) = match src {
            Source::General { .. } => self.reference.clone(),
            Source::EvaniModel { input } => {
                let p = self.dir.join(format!("{tier}.input.glb"));
                std::fs::copy(input, &p).map_err(|e| format!("copy {} to {}: {e}", input.display(), p.display()))?;
                (p, format!("the {tier} input (Evani.Model)"))
            }
        };
        let refs = self.render(tier, &reference_path, &format!("{tier}-reference"))?;
        check_drawn(&refs, &format!("{}'s {tier} reference ({reference})", self.id))?;
        let render =refs.first().map_or((0, 0), |r| (r.w, r.h));
        let start = knobs.at(&[0; 5]);
        let want_pairs = self.pairs.is_some();
        let mut problems = Vec::new();
        // A setting's renders: one per texture family its candidate has, in
        // `order`; with a `bar`, stopping at the first family that fails it.
        let try_setting = |this: &mut Gate, st: &Setting, order: &[&'static str], bar: Option<f64>| -> Result<Trial, String> {
            let (_, mut files) = this.candidate(tier, src, st)?;
            files.sort_by_key(|(f, _)| f.and_then(|f| order.iter().position(|o| *o == f)).unwrap_or(0));
            let mut families = Vec::new();
            let mut pairs = Vec::new();
            for (family, p) in files {
                let name = match family {
                    Some(f) => format!("{tier}-{}.{f}", st.key()),
                    None => format!("{tier}-{}", st.key()),
                };
                let got = this.render(tier, &p, &name)?;
                let views = score_views(&rig, &refs, &got)?;
                let (ssim, worst, k) = worst_of(&views);
                pairs.push(want_pairs.then(|| pair_crop(&rig, &got, &views, k)));
                families.push(FamilyViews { family, views });
                if bar.is_some_and(|b| !passes_at(ssim, worst, b)) {
                    break;
                }
            }
            let w = worst_family(&families).unwrap_or(0);
            let views = families.get(w).map(|f| f.views.clone()).unwrap_or_default();
            let pair = pairs.get_mut(w).and_then(Option::take);
            Ok(Trial { setting: st.clone(), families, views, pass: false, pair })
        };
        // The best rung first, on every family: its worst family's score is
        // the format floor that sets the bar.
        let floor_trial = try_setting(self, &start, &FAMILIES, None)?;
        let (floor, floor_worst, _) = worst_of(&floor_trial.views);
        let bar = bar_for(floor, floor_worst);
        let floor_used = floor_counts(floor, floor_worst);
        eprintln!("  gate {tier}: format floor (the best rung, worst texture family) SSIMULACRA2 {floor:.1}, worst pixel {floor_worst:.3}; bar {bar:.1}");
        let mut first = Some(floor_trial);
        let mut trials: Vec<Trial> = Vec::new();
        // The family that failed last is tried first next time.
        let mut order: Vec<&'static str> = FAMILIES.to_vec();
        let res = search(&knobs.lens(), &mut |idx| {
            let st = knobs.at(idx);
            let mut tr = match (idx.iter().all(|i| *i == 0), first.take()) {
                (true, Some(f)) => f,
                (_, f) => {
                    first = f;
                    try_setting(self, &st, &order, Some(bar))?
                }
            };
            tr.pass = families_pass(&tr.families, bar);
            let (ssim, worst, k) = worst_of(&tr.views);
            let fam = worst_family(&tr.families).and_then(|w| tr.families[w].family);
            if !tr.pass
                && let Some(f) = tr.families.last().and_then(|f| f.family)
                && let Some(i) = order.iter().position(|o| *o == f)
            {
                let f = order.remove(i);
                order.insert(0, f);
            }
            let on = fam.map_or(String::new(), |f| format!(", {f}"));
            eprintln!("  gate {tier}: {} -> SSIMULACRA2 {ssim:.1}, worst pixel {worst:.3} ({}{on}): {}", st.describe(), rig.views[k].name, if tr.pass { "pass" } else { "fail" });
            let pass = tr.pass;
            trials.push(tr);
            Ok(pass)
        })?;
        let chosen = res.map(|i| knobs.at(&i));
        if chosen.is_none() {
            problems.push(format!("{}.{tier}.glb: the quality gate fails even at the best setting on the ladder ({}); the file keeps the tier's own settings ({})", self.id, start.describe(), knobs.own.describe()));
        }
        self.tiers.push(TierGate { tier: tier.to_owned(), window: (w, h), render, reference, refs, start, own: knobs.own.clone(), floor, floor_used, bar, chosen: chosen.clone(), trials, verified: None, drift: None, geometry_same: None, sheet: None, problems });
        Ok(chosen)
    }

    /// The finished tier file as each device texture family draws it: for
    /// a file with `extras.evani.textures`, one copy per family in the gate
    /// folder (`{tier}-final.{family}.glb`) whose images are that family's
    /// own files from the output, decoded (`final-{tier}/`), with the sets
    /// list removed so every device samples those images; for any other
    /// file, the file itself (family `None`).
    fn final_variants(&self, tier: &str, file: &Path) -> Result<Vec<FamilyFile>, String> {
        let doc = glb::Doc::read_file(file)?;
        let sets = doc.json.pointer("/extras/evani/textures").and_then(Value::as_array).cloned().unwrap_or_default();
        if sets.is_empty() {
            return Ok(vec![(None, file.to_path_buf())]);
        }
        let base = file.parent().unwrap_or(&self.out).to_path_buf();
        let sub = format!("final-{tier}");
        let dir = self.dir.join(&sub);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        // Per image: its name and device sets (family -> file, relative to the tier file).
        let mut image_sets: BTreeMap<usize, (String, BTreeMap<String, String>)> = BTreeMap::new();
        for e in &sets {
            let Some(t) = e.get("texture").and_then(Value::as_u64) else { continue };
            let Some(im) = doc.json.pointer(&format!("/textures/{t}/source")).and_then(Value::as_u64) else { continue };
            let name = e.get("name").and_then(Value::as_str).unwrap_or("image").to_owned();
            let m: BTreeMap<String, String> = e.get("sets").and_then(Value::as_object).into_iter().flatten().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned()))).collect();
            image_sets.insert(im as usize, (name, m));
        }
        let mut out = Vec::new();
        for family in FAMILIES {
            let mut d = glb::Doc::read_file(file)?;
            if let Some(Value::Array(images)) = d.json.get_mut("images") {
                for (i, im) in images.iter_mut().enumerate() {
                    let Some(uri) = im.get("uri").and_then(Value::as_str).map(str::to_owned) else { continue };
                    let core = base.join(glb::uri_path(&uri));
                    let stem = core.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_owned();
                    let set = image_sets.get(&i).and_then(|(name, m)| m.get(family).map(|f| (name, f)));
                    let (rel_name, mime) = match set {
                        Some((name, f)) if family != "rgba" => {
                            let n = format!("{sub}/{name}.{family}.png");
                            decode_set(&self.tools, family, &base.join(glb::uri_path(f)), &self.dir.join(&n))?;
                            (n, Value::from("image/png"))
                        }
                        _ => {
                            let n = format!("{sub}/{stem}");
                            let to = self.dir.join(&n);
                            std::fs::copy(&core, &to).map_err(|e| format!("copy {} to {}: {e}", core.display(), to.display()))?;
                            (n, im.get("mimeType").cloned().unwrap_or(Value::Null))
                        }
                    };
                    im["uri"] = Value::from(rel_name);
                    if !mime.is_null() {
                        im["mimeType"] = mime;
                    }
                }
            }
            if let Some(Value::Object(o)) = d.json.pointer_mut("/extras/evani") {
                o.remove("textures");
            }
            let fp = self.dir.join(format!("{tier}-final.{family}.glb"));
            std::fs::write(&fp, d.to_glb()?).map_err(|e| format!("{}: {e}", fp.display()))?;
            out.push((Some(family), fp));
        }
        Ok(out)
    }

    /// Render the finished tier file (each device texture family's view of
    /// it), score it again and write its sheet (the worst family's).
    pub fn verify(&mut self, tier: &str, file: &Path) -> Result<(), String> {
        let (w, h) = window(&self.root, tier)?;
        let rig = self.rig(w, h);
        let Some(k) = self.tiers.iter().position(|t| t.tier == tier) else { return Err(format!("gate: {tier} was not tuned")) };
        let mut families = Vec::new();
        let mut gots = Vec::new();
        for (family, p) in self.final_variants(tier, file)? {
            let name = match family {
                Some(f) => format!("{tier}-final.{f}"),
                None => format!("{tier}-final"),
            };
            let got = self.render(tier, &p, &name)?;
            let views = score_views(&rig, &self.tiers[k].refs, &got)?;
            families.push(FamilyViews { family, views });
            gots.push(got);
        }
        let fw = worst_family(&families).unwrap_or(0);
        let views = families.get(fw).map(|f| f.views.clone()).unwrap_or_default();
        if fw >= gots.len() {
            return Err(format!("gate: no render of the finished {tier} file"));
        }
        let got = gots.swap_remove(fw);
        let (ssim, worst, wk) = worst_of(&views);
        let bar = self.tiers[k].bar;
        let pass = families_pass(&families, bar);
        let on = families.get(fw).and_then(|f| f.family).map_or(String::new(), |f| format!(", worst on {f}"));
        eprintln!("  gate {tier}: the tier file -> SSIMULACRA2 {ssim:.1}, worst pixel {worst:.3}{on} (bar {bar:.1}): {}", if pass { "pass" } else { "fail" });
        let sheet = sheet(&rig, &self.tiers[k].refs, &got, &views)?;
        let name = format!("gate/{}.{tier}.sheet.png", self.id);
        let sp = self.out.join(&name);
        if let Some(d) = sp.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(&sp, sheet).map_err(|e| format!("{}: {e}", sp.display()))?;
        // The file's setting: the chosen one, or the tier's own when nothing
        // passed. Its candidate (still in the gate's folder, when the search
        // tried it) and the finished file must carry the same geometry,
        // textured or not.
        let setting = self.tiers[k].chosen.clone().unwrap_or_else(|| self.tiers[k].own.clone());
        let cand = self.dir.join(format!("{tier}-{}.glb", setting.key()));
        let geometry = if cand.is_file() { Some(geometry_difference(&glb::Doc::read_file(&cand)?.json, &glb::Doc::read_file(file)?.json)) } else { None };
        let t = &mut self.tiers[k];
        t.geometry_same = geometry.as_ref().map(Option::is_none);
        if let Some(Some(d)) = geometry {
            t.problems.push(format!("{}.{tier}.glb: the finished file's geometry is not the gate's candidate of the same setting ({d}), so the search judged other geometry than ships", self.id));
        }
        if !pass {
            let failing: Vec<&str> = families
                .iter()
                .filter(|f| {
                    let (s2, w2, _) = worst_of(&f.views);
                    !passes_at(s2, w2, bar)
                })
                .map(|f| f.family.unwrap_or("untextured"))
                .collect();
            t.problems.push(format!(
                "{}.{tier}.glb: the quality gate fails on the finished file (worst view {}: SSIMULACRA2 {ssim:.1}, worst pixel {worst:.3}{on}; failing on {}; bar {bar:.1} and {WORST_MAX}, {THRESHOLDS_STATUS})",
                self.id,
                rig.views[wk].name,
                failing.join(", ")
            ));
        }
        // The gate's own trial of this setting should look like the file it
        // chose, family by family.
        if let Some(tr) = t.trials.iter().find(|tr| tr.setting == setting)
            && let Some((drop, family, was, now)) = family_drift(&tr.families, &families)
        {
            t.drift = Some(drop);
            if drifted(was, now) {
                let on = family.map_or(String::new(), |f| format!(" ({f} textures)"));
                t.problems.push(format!(
                    "{}.{tier}.glb: the finished file scores SSIMULACRA2 {now:.1}{on}, {drop:.1} under the gate's trial of the same setting ({was:.1}): the tier file is not made the way the gate's candidates are, so the search judged something else",
                    self.id
                ));
            }
        }
        let pair = self.pairs.is_some().then(|| pair_crop(&rig, &got, &views, wk));
        t.verified = Some(Trial { setting, families, views, pass, pair });
        t.sheet = Some(name);
        Ok(())
    }

    /// The gate's part of the report (and, with `--gate-pairs`, the
    /// calibration pairs). Stops the runner.
    pub fn finish(self) -> Result<Value, String> {
        let pairs_written = match &self.pairs {
            Some(dir) => self.write_pairs(dir)?,
            None => 0,
        };
        let mut tiers = Vec::new();
        let mut problems: Vec<String> = Vec::new();
        for t in &self.tiers {
            let rig = self.rig(t.window.0, t.window.1);
            problems.extend(t.problems.iter().cloned());
            let trial_json = |tr: &Trial| {
                let (ssim, worst, k) = worst_of(&tr.views);
                let families: serde_json::Map<String, Value> = tr
                    .families
                    .iter()
                    .filter_map(|f| {
                        let (s2, w2, _) = worst_of(&f.views);
                        f.family.map(|n| (n.to_owned(), json!({ "ssimulacra2": round(s2, 2), "worst_pixel": round(w2, 4) })))
                    })
                    .collect();
                let family = worst_family(&tr.families).and_then(|w| tr.families[w].family);
                json!({ "setting": tr.setting.to_json(), "pass": tr.pass, "worst_view": rig.views.get(k).map(|v| v.name), "ssimulacra2": round(ssim, 2), "worst_pixel": round(worst, 4), "family": family, "families": families })
            };
            tiers.push(json!({
                "tier": t.tier,
                "window": [t.window.0, t.window.1],
                "render": [t.render.0, t.render.1],
                "reference": t.reference,
                "start": t.start.to_json(),
                "own": t.own.to_json(),
                "floor": round(t.floor, 2),
                "floor_used": t.floor_used,
                "ssimulacra2_bar": round(t.bar, 2),
                "chosen": t.chosen.as_ref().map(Setting::to_json),
                "trials": t.trials.iter().map(trial_json).collect::<Vec<_>>(),
                "final": t.verified.as_ref().map(|v| {
                    let mut j = trial_json(v);
                    j["views"] = Value::Array(views_json(&rig, &v.views));
                    j
                }),
                "drift": t.drift.map(|d| round(d, 2)),
                "geometry_same": t.geometry_same,
                "sheet": t.sheet,
            }));
        }
        let names: Vec<&str> = self.rig(1920, 1080).views.iter().map(|v| v.name).collect();
        Ok(json!({
            "thresholds": { "ssimulacra2_min": SSIM_MIN, "worst_pixel_max": WORST_MAX, "floor_slack": FLOOR_SLACK, "floor_min": FLOOR_MIN, "bar": "min(ssimulacra2_min, floor - floor_slack) per tier, the floor being the best rung's worst view (when it is at least floor_min and within worst_pixel_max)", "on": "the worst of the views", "status": THRESHOLDS_STATUS },
            "renderer": format!("apps/model-view gate page (toon look, compiled effects), {}", self.runner.chrome.trim()),
            "views": names,
            "place": self.rig(1920, 1080).place,
            "renders": self.renders,
            "seconds": round(self.started.elapsed().as_secs_f64(), 1),
            "tiers": tiers,
            "calibration_pairs": pairs_written,
            "problems": problems,
        }))
    }

    /// About five pairs spanning this model's scores plus one identical
    /// control, merged into `<dir>/key.json`.
    fn write_pairs(&self, dir: &Path) -> Result<usize, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut all: Vec<(&TierGate, &Trial)> = Vec::new();
        for t in &self.tiers {
            for tr in t.trials.iter().chain(t.verified.iter()) {
                if tr.pair.is_some() {
                    all.push((t, tr));
                }
            }
        }
        // Distinct settings only, by their worst SSIMULACRA2.
        all.sort_by(|a, b| worst_of(&a.1.views).0.total_cmp(&worst_of(&b.1.views).0));
        let n = all.len();
        let mut pick: Vec<usize> = if n == 0 { Vec::new() } else { (0..5).map(|q| q * (n - 1) / 4).collect() };
        pick.dedup();
        let mut new_pairs = Vec::new();
        let write = |name: &str, img: &Rgba| -> Result<(), String> {
            let p = dir.join(name);
            std::fs::write(&p, png_rgb(img.w, img.h, &rgb_of(img))?).map_err(|e| format!("{}: {e}", p.display()))
        };
        for (k, &i) in pick.iter().enumerate() {
            let (t, tr) = all[i];
            let Some((view, r, b)) = &tr.pair else { continue };
            let a = t.refs[*view].crop(*r);
            let (ssim, worst, _) = worst_of(&tr.views);
            let pid = format!("{}-{}-{k}", self.id, t.tier);
            write(&format!("{pid}-a.png"), &a)?;
            write(&format!("{pid}-b.png"), b)?;
            new_pairs.push(json!({ "id": pid, "a": format!("{pid}-a.png"), "b": format!("{pid}-b.png"), "model": self.id, "tier": t.tier, "view": self.rig(t.window.0, t.window.1).views[*view].name, "setting": tr.setting.to_json(), "ssimulacra2": round(ssim, 2), "worst_pixel": round(worst, 4), "floor": round(t.floor, 2), "control": false }));
        }
        // The control: the original against itself.
        if let Some((t, tr)) = all.first()
            && let Some((view, r, _)) = &tr.pair
        {
            let a = t.refs[*view].crop(*r);
            let pid = format!("{}-{}-control", self.id, t.tier);
            write(&format!("{pid}-a.png"), &a)?;
            write(&format!("{pid}-b.png"), &a)?;
            new_pairs.push(json!({ "id": pid, "a": format!("{pid}-a.png"), "b": format!("{pid}-b.png"), "model": self.id, "tier": t.tier, "view": self.rig(t.window.0, t.window.1).views[*view].name, "setting": Value::Null, "ssimulacra2": 100.0, "worst_pixel": 0.0, "control": true }));
        }
        let kf = dir.join("key.json");
        let mut pairs: Vec<Value> = match std::fs::read_to_string(&kf) {
            Ok(t) => serde_json::from_str::<Value>(&t).map_err(|e| format!("{}: {e}", kf.display()))?["pairs"].as_array().cloned().unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        pairs.retain(|p| p["model"].as_str() != Some(self.id.as_str()));
        let written = new_pairs.len();
        pairs.extend(new_pairs);
        let key = json!({ "format": 1, "about": "Blind-test pairs for the quality gate (STAGE3_PLAN S3-6, decision 4): a is the original render, b the converted one; the page shows them in a random order and never shows these scores.", "pairs": pairs });
        std::fs::write(&kf, serde_json::to_string_pretty(&key).map_err(|e| e.to_string())? + "\n").map_err(|e| format!("{}: {e}", kf.display()))?;
        Ok(written)
    }
}

/// Decode a device set's level 0 to an RGBA PNG: ASTC with `ktx extract`
/// (its reference decoder), BC7 with [`crate::bc7`] (`ktx extract` refuses
/// BC7). The PNG holds the stored values, sRGB or linear as the set is,
/// which is what the engine's colour-conversion-free image upload samples.
fn decode_set(tools: &Tools, family: &str, ktx: &Path, png: &Path) -> Result<(), String> {
    match family {
        "astc" => tools.ktx(&["extract", &s(ktx), &s(png)], &format!("ktx extract ASTC {}", ktx.display())).map(|_| ()),
        "bc7" => {
            let bytes = std::fs::read(ktx).map_err(|e| format!("{}: {e}", ktx.display()))?;
            let (w, h, px) = crate::bc7::decode_ktx2_level0(&bytes).map_err(|e| format!("{}: {e}", ktx.display()))?;
            std::fs::write(png, png_rgba(w, h, &px)?).map_err(|e| format!("{}: {e}", png.display()))
        }
        f => Err(format!("gate: no decoder for the {f} texture family")),
    }
}

/// The worst view's model crop, at 1:1, at most 1200 x 800 around its worst pixel.
fn pair_crop(rig: &Rig, got: &[Rgba], views: &[ViewScore], k: usize) -> (usize, [usize; 4], Rgba) {
    let b = &got[k];
    let c = crop_px(rig.views[k].crop, b.w, b.h);
    let (cw, ch) = (c[2].min(1200), c[3].min(800));
    let at = views[k].at;
    let x = at[0].saturating_sub(cw / 2).clamp(c[0], c[0] + c[2] - cw);
    let y = at[1].saturating_sub(ch / 2).clamp(c[1], c[1] + c[3] - ch);
    let r = [x, y, cw, ch];
    (k, r, b.crop(r))
}

// ------------------------------------------------------------ calibration

/// `--calibration <folder>`: serve the blind test page until stopped.
pub fn calibration_page(root: &Path, dir: &Path) -> Result<(), String> {
    let tools = Tools::locate(root)?;
    let script = root.join("tools").join("convert-runner").join("run.mjs");
    let st = std::process::Command::new(&tools.node).arg(&script).arg("--calibrate").arg(dir).status().map_err(|e| format!("cannot start node ({e})"))?;
    if st.success() { Ok(()) } else { Err(format!("the calibration page stopped ({st})")) }
}

/// `--calibration-result <folder>`: thresholds from Chris's answers. A pair
/// judged "different" must fail; one judged "same" may pass. The suggested
/// SSIMULACRA2 floor sits just above every pair seen as different, and the
/// worst-pixel ceiling just below them.
pub fn calibration_result(dir: &Path) -> Result<Value, String> {
    let read = |n: &str| -> Result<Value, String> {
        let p = dir.join(n);
        serde_json::from_str(&std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| format!("{}: {e}", p.display()))
    };
    let key = read("key.json")?;
    let ans = read("answers.json")?;
    let answers: BTreeMap<String, String> = ans["answers"].as_array().into_iter().flatten().filter_map(|a| Some((a["id"].as_str()?.to_owned(), a["answer"].as_str()?.to_owned()))).collect();
    let (mut seen_diff, mut seen_same, mut controls_wrong, mut controls) = (Vec::new(), Vec::new(), 0, 0);
    for p in key["pairs"].as_array().into_iter().flatten() {
        let Some(a) = p["id"].as_str().and_then(|id| answers.get(id)) else { continue };
        if p["control"].as_bool() == Some(true) {
            controls += 1;
            if a == "different" {
                controls_wrong += 1;
            }
            continue;
        }
        let ssim = p["ssimulacra2"].as_f64().unwrap_or(0.0);
        // Pairs from before the floor rule carry no floor: read them as at the absolute bar.
        let floor = p["floor"].as_f64().unwrap_or(SSIM_MIN + FLOOR_SLACK);
        let sc = (ssim, p["worst_pixel"].as_f64().unwrap_or(1.0), floor - ssim);
        if a == "different" { seen_diff.push(sc) } else { seen_same.push(sc) }
    }
    let ssim_floor = seen_diff.iter().map(|x| x.0).fold(f64::MIN, f64::max);
    let worst_ceiling = seen_diff.iter().map(|x| x.1).fold(f64::MAX, f64::min);
    // A pair seen as different must sit below its floor by more than the slack.
    let below_floor = seen_diff.iter().map(|x| x.2).fold(f64::MAX, f64::min);
    let suggest_ssim = if seen_diff.is_empty() { None } else { Some(round(ssim_floor + 0.5, 1)) };
    let suggest_worst = if seen_diff.is_empty() { None } else { Some(round(worst_ceiling * 0.95, 3)) };
    let suggest_slack = if seen_diff.is_empty() { None } else { Some(round((below_floor - 0.5).max(0.0), 1)) };
    Ok(json!({
        "answered": answers.len(),
        "pairs": key["pairs"].as_array().map_or(0, Vec::len),
        "judged_different": seen_diff.len(),
        "judged_same": seen_same.len(),
        "controls": controls,
        "controls_judged_different": controls_wrong,
        "current": { "ssimulacra2_min": SSIM_MIN, "worst_pixel_max": WORST_MAX, "floor_slack": FLOOR_SLACK },
        "suggested": { "ssimulacra2_min": suggest_ssim, "worst_pixel_max": suggest_worst, "floor_slack": suggest_slack },
        "note": if controls_wrong > 0 { "a control (identical pictures) was judged different: the answers are noisy, so take the suggestion as a rough guide" } else { "suggested thresholds sit just past every pair judged different; put them into gate.rs (SSIM_MIN, WORST_MAX, FLOOR_SLACK) and DECISIONS once Chris agrees" },
    }))
}
