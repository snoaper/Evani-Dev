//! The field sea (`material.nature.water.wave_field`, and every storm sea:
//! any of `mid_waves`, `foam_streaks`, `spindrift` above 0): the sea drawn
//! from wave textures made each frame on the GPU and a camera-tied mesh
//! that moves by them, instead of the classic per-pixel march. Its cost
//! doesn't grow with the waves' height (the classic march's steps do), and
//! its crests are squeezed sideways into peaks (Gerstner), as Ultra-Water's
//! are. The classic sea, its shader and its pixels are exactly as they were
//! for every sea that doesn't opt in.
//!
//! Per frame: `nature_sea_waves` fills the small atlas [`FIELD`] (8 bands of
//! waves, each 2.83x shorter than the last, on periodic tiles; layout
//! `shaders/sea_field_bands.wgsl`) from the field table
//! ([`spectrum::field_table`]); `nature_sea_field` draws the mesh
//! ([`GRID`]) with its own depth [`ZBUF`]; `nature_sea_fill` draws the
//! water under the surface and a beach's swash when needed. This module
//! packs their uniform words ([`Field::new`]: the classic words plus the
//! squeeze, the longest wave, the foam's folding and the unresolved
//! roughness) and mirrors the waves on the CPU ([`Field::height_at`]: the
//! deck camera, the tests).
//!
//! Ideas from Ultra-Water (Humpalumps, MIT; DECISIONS.md "Field sea"):
//! precomputed cascades read by a displaced mesh, the short-wave fade, the
//! Jacobian foam, the roughness stack, the far haze. Our own code, a direct
//! sum (no FFT).

use super::{FrameIn, Settings, TAU, classic_uniforms, spectrum};

/// The wave textures (a fixed-size target) and the mesh's own depth.
pub const FIELD: &str = "nature.sea.field";
pub const ZBUF: &str = "nature.sea.zbuf";
/// Bands, texels across a band's tile (plus a one-texel wrap border).
pub const BANDS: usize = 8;
pub const FN: usize = 128;
pub const FT: usize = FN + 2;
/// The atlas: tiles A, B and C side by side, the bands down, one row of band
/// numbers at the bottom (`shaders/sea_field_bands.wgsl`).
pub const FIELD_SIZE: [u32; 2] = [3 * FT as u32, (BANDS * FT + 1) as u32];
/// Each band's waves are this much shorter than the last's.
pub const RHO: f32 = 2.828_427;
/// A band's tile: this many of its longest waves across, times its own
/// factor (so the tiles never line up).
pub const TILE: f32 = 8.0;
pub const JIT: [f32; BANDS] = [1.0, 1.09, 0.93, 1.13, 0.97, 1.06, 0.91, 1.11];
/// Bands per level (Low, Medium, High, Ultra).
pub const BANDS_PER_LEVEL: [usize; 4] = [6, 7, 8, 8];
/// The mesh per level: rings, segments (the shader's `fd_grid`).
pub const GRID: [(u32, u32); 4] = [(160, 192), (384, 384), (576, 576), (768, 768)];
/// The splash per level: cells across the grid round the camera, puffs a
/// cell (the shader's `sp_grid`; Low has none).
pub const SPRAY: [(u32, u32); 4] = [(0, 0), (12, 10), (20, 28), (20, 48)];
/// The squeeze is held so its folding stays under this many of the
/// surface's rms slopes (sharp crests, no spikes folding through: not
/// Ultra-Water's "liquid metal" peaks in a rough sea).
pub const FOLD_CAP: f32 = 0.45;
/// The foam's folding: stronger than the geometry's, held as above.
pub const FOAM_CAP: f32 = 0.55;
/// How much taller the waves under about 40 m stand (the shader's waves draw).
pub const SHORT_GAIN: f32 = 0.35;
/// The short waves' fade on their amplitude, `exp(-FIELD_FADE k^2)` (the wave
/// pass's `FIELD_FADE`): Ultra-Water's short-wave fade (0.04 m on the energy),
/// so 0.0008 on the amplitude; twice that left wave faces near the eye without
/// their centimetre ripples (review round 6 r2).
pub const FIELD_FADE: f32 = 0.0008;
/// The choppiness param's scale to the squeeze (1 = Ultra-Water's hosted
/// 1.14, about).
pub const CHOP: f32 = 1.2;
/// The mesh's squeeze eases off past these stretches of lambda M (the shader's
/// `FD_CMAX`, `FD_CMIN`): along its strongest axis (no fold), and along its
/// weakest (no lone pyramid where it bunches the water from every side).
pub const SQUEEZE_CMAX: f32 = 0.65;
pub const SQUEEZE_CMIN: f32 = 0.18;

/// The long bands (the first three), whose squeeze is capped by their own
/// stretch tensor; the rest get what that leaves (the shader's `FD_LONG`). One
/// cap read off every band varies over the short waves' few metres, and
/// scaling a storm's tens of metres of long-wave squeeze by it folded the mesh
/// into stacked shelves beside a tall crest.
pub const SQUEEZE_LONG: usize = 3;

/// How much of the squeeze the mesh keeps where q lambda M is `lm` (xx, zz,
/// xz): the shader's `fd_squeeze_cap`.
pub fn squeeze_cap(lm: [f32; 3]) -> f32 {
    let cap = |e: f32, c: f32| {
        let r = e.max(0.0) / c;
        (1.0 + r * r * r * r).powf(-0.25)
    };
    let mean = 0.5 * (lm[0] + lm[1]);
    let dev = (0.25 * (lm[0] - lm[1]) * (lm[0] - lm[1]) + lm[2] * lm[2]).sqrt();
    cap(mean + dev, SQUEEZE_CMAX).min(cap(mean - dev, SQUEEZE_CMIN))
}
/// Bits of the storm code (above mid waves, streaks and spindrift): the
/// field sea is on; the eye is under the water.
pub const FIELD_BIT: u32 = 1 << 20;
pub const UNDER_BIT: u32 = 1 << 21;
/// The foam view's switch in the spray word (dev builds: `foam_debug`;
/// sea_field_debug.wgsl).
pub const FOAM_DEBUG_BIT: u32 = 1 << 16;

/// The field table the wave textures read (`spectrum::field_table_wgsl`).
pub const FIELD_TABLE_WGSL: &str = include_str!("../../shaders/sea_field_table.wgsl");

/// Whether `s` draws the field sea.
pub fn field_on(s: &Settings) -> bool {
    s.wave_field > 0.5 || s.mid_waves > 0.0 || s.foam_streaks > 0.0 || s.spindrift > 0.0 || s.breaking > 0.0
}

/// The mesh's vertices at `level` (6 per quad, `GRID`).
pub fn vertices(level: usize) -> u32 {
    let (r, s) = GRID[level.min(3)];
    6 * (r - 1) * s
}

/// The chance a cell's crest breaks in a cycle: more as the wind rises,
/// none in a light breeze, never every wave.
pub fn brk_chance(s: &Settings) -> f32 {
    (s.breaking.max(0.0) * smoothstep(7.0, 22.0, s.wind_speed)).clamp(0.0, 0.9)
}

/// The splash's vertices at `level` (6 per puff, `SPRAY`).
pub fn spray_vertices(level: usize) -> u32 {
    let (g, n) = SPRAY[level.min(3)];
    6 * g * g * n
}

/// The storm settings in the level word (as before: `level + 4 code`, each
/// in steps of 0.02; exact in f32, under 2^24 with the two flag bits).
pub fn storm_code(s: &Settings) -> u32 {
    let q = |v: f32, max: u32| ((v.max(0.0) / 0.02).round() as u32).min(max);
    q(s.mid_waves, 127) | (q(s.foam_streaks, 63) << 7) | (q(s.spindrift, 127) << 13)
}

/// The field table as the shader reads it (parsed from the WGSL text).
pub fn table() -> Vec<spectrum::Comp> {
    let mut comps = Vec::new();
    let mut phases = Vec::new();
    let mut part = 0;
    for line in FIELD_TABLE_WGSL.lines() {
        let l = line.trim();
        if l.starts_with("const FIELD_C") {
            part = 1;
            continue;
        }
        if l.starts_with("const FIELD_P") {
            part = 2;
            continue;
        }
        if l.starts_with(");") {
            part = 0;
            continue;
        }
        let nums = |s: &str| -> Vec<f32> { s.split(',').filter_map(|x| x.trim().trim_end_matches(')').parse::<f32>().ok()).collect() };
        match part {
            1 => {
                if let Some(inner) = l.strip_prefix("vec4<f32>(") {
                    let v = nums(inner);
                    if v.len() == 4 {
                        comps.push(spectrum::Comp { xx: v[0], cos: v[1], sin: v[2], amp: v[3], phase: 0.0 });
                    }
                }
            }
            2 => phases.extend(nums(l)),
            _ => {}
        }
    }
    for (c, p) in comps.iter_mut().zip(phases) {
        c.phase = p;
    }
    comps
}

thread_local! {
    // (Shared: `Field::new` runs a few times a frame.)
    static TABLE: std::rc::Rc<Vec<spectrum::Comp>> = std::rc::Rc::new(table());
}

/// One component as a band holds it: snapped wave vector, amplitude, phase
/// now, band (`BANDS`: in none).
#[derive(Debug, Clone, Copy, Default)]
struct Fc {
    k: [f32; 2],
    a: f32,
    ph: f32,
    band: usize,
    // The squeeze's share (`chop_of`).
    cf: f32,
}

/// One frame's field sea: its uniform words and its waves on the CPU.
#[derive(Debug, Clone)]
pub struct Field {
    pub words: [f32; 64],
    pub level: usize,
    /// The squeeze, the foam's folding, the longest wave (m).
    pub lam: f32,
    pub lamf: f32,
    pub hi0: f32,
    /// The rms height (m) and the slope variance (both axes) the bands hold.
    pub hrms: f32,
    pub slope_var: f32,
    pub eye_under: bool,
    /// Each band's rms height (m).
    brms: [f32; BANDS],
    centre: [f32; 2],
    depth: f32,
    comps: Vec<Fc>,
}

/// How much of the squeeze a wave of k / k_p = `x` gets (the shader's
/// `f_chop`): long waves round their crests, the chop keeps its edges.
pub fn chop_of(x: f32) -> f32 {
    0.55 + 0.45 * smoothstep(1.5, 6.0, x)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Band `b`'s longest wave for a sea whose longest is `hi0`.
pub fn band_hi(hi0: f32, b: usize) -> f32 {
    hi0 / RHO.powi(b as i32)
}

/// Band `b`'s tile length.
pub fn tile(hi0: f32, b: usize) -> f32 {
    TILE * band_hi(hi0, b) * JIT[b.min(BANDS - 1)]
}

impl Field {
    pub fn new(s: &Settings, fr: &FrameIn) -> Self {
        let mut w = classic_uniforms(s, fr);
        let level = fr.level.min(3);
        let nb = BANDS_PER_LEVEL[level];
        let (kpw, aw, dw, kps, as_, ds) = (w[32], w[33], w[34], w[35], w[36], w[37]);
        // The longest wave the sea makes, a little over, up to a step of
        // 2^(1/4) (so a slider's small change doesn't reshuffle the tiles).
        let t = TABLE.with(std::rc::Rc::clone);
        let xx_w = t.iter().take(spectrum::FIELD_WIND).map(|c| c.xx).fold(f32::MAX, f32::min);
        let xx_s = t.iter().skip(spectrum::FIELD_WIND).map(|c| c.xx).fold(f32::MAX, f32::min);
        let mut longest = TAU / (kpw * xx_w).max(1e-6);
        if as_ > 0.0 {
            longest = longest.max(TAU / (kps * xx_s).max(1e-6));
        }
        let hi0 = ((longest * 1.02).max(1.0).log2() * 4.0).ceil() * 0.25;
        let hi0 = hi0.exp2();
        let mid = (storm_code(s) & 127) as f32 * 0.02;
        let gm = if mid > 0.0 { mid } else { 1.0 };
        let (rw, rs) = ((dw.cos(), dw.sin()), (ds.cos(), ds.sin()));
        let tl = w[11];
        let mut comps = Vec::with_capacity(t.len());
        let (mut h2, mut sv) = (0.0f32, 0.0f32);
        let mut b2 = [0.0f32; BANDS];
        for (i, c) in t.iter().enumerate() {
            let swell = i >= spectrum::FIELD_WIND;
            let mut fc = Fc { band: BANDS, ..Fc::default() };
            if level < 2 && !swell && i % 2 == 1 {
                comps.push(fc);
                continue;
            }
            let (kp, a0, r) = if swell { (kps, as_, rs) } else { (kpw, aw, rw) };
            let k = kp * c.xx;
            let lam = TAU / k.max(1e-6);
            let band = if lam > hi0 / RHO { 0 } else { ((hi0 / lam).ln() / RHO.ln()).floor() as usize };
            // (The shader's interval test, exactly: hi_b / rho < lam <= hi_b.)
            let band = (band.saturating_sub(1)..=band + 1).find(|&b| (b == 0 || lam <= band_hi(hi0, b)) && lam > band_hi(hi0, b) / RHO).unwrap_or(BANDS);
            if band >= nb || a0 <= 0.0 {
                comps.push(fc);
                continue;
            }
            let d = [r.0 * c.cos - r.1 * c.sin, r.1 * c.cos + r.0 * c.sin];
            let dk = TAU / tile(hi0, band);
            let ks = [(d[0] * k / dk).round() * dk, (d[1] * k / dk).round() * dk];
            let km = (ks[0] * ks[0] + ks[1] * ks[1]).sqrt();
            if km <= 0.0 {
                comps.push(fc);
                continue;
            }
            let mut a = a0 * c.amp * (-FIELD_FADE * km * km).exp();
            if !swell {
                a *= 1.0 + (gm - 1.0) * smoothstep(2.0, 2.56, c.xx) * (1.0 - smoothstep(36.0, 46.0, c.xx));
                if level < 2 {
                    a *= core::f32::consts::SQRT_2;
                }
                // Measured against Ultra-Water by wavelength
                // (tools/nature/uw-probe.mjs): at the mapped height its waves
                // from 16 m to 1 km match ours, those of 4 to 16 m stand 20 to
                // 50 % taller.
                a *= 1.0 + SHORT_GAIN * smoothstep(0.1, 0.4, km);
            }
            let n = ((9.81 * km).sqrt() / super::W0).round().max(1.0);
            fc = Fc { k: ks, a, ph: TAU * (c.phase - (n * tl).fract()), band, cf: chop_of(c.xx) };
            h2 += 0.5 * a * a;
            b2[band] += 0.5 * a * a;
            sv += 0.5 * a * a * km * km;
            comps.push(fc);
        }
        let hrms = h2.sqrt();
        let sig = sv.sqrt().max(1e-4);
        let lam = (CHOP * s.choppiness.max(0.0)).min(FOLD_CAP / sig);
        let lamf = (1.0 + 0.6 * s.choppiness.max(0.0)).min(FOAM_CAP / sig).max(lam);
        let v_unres = (0.003 + 0.00512 * s.wind_speed - sv).max(0.0);
        let mut f = Field {
            words: [0.0; 64],
            level,
            lam,
            lamf,
            hi0,
            hrms,
            slope_var: sv,
            eye_under: false,
            brms: b2.map(f32::sqrt),
            centre: fr.sea.centre,
            depth: s.depth.max(0.5),
            comps,
        };
        let eye = fr.camera.eye;
        let inside = (eye[0] - fr.sea.centre[0]).abs() <= fr.sea.half[0] && (eye[2] - fr.sea.centre[1]).abs() <= fr.sea.half[1];
        // The mesh's rings go round the water the squeeze carries under the eye
        // (the shader's `fd_centre`; choppiness is the classic sea's word).
        let rest = if level > 0 { f.rest_under([eye[0], eye[2]], 0.0) } else { [eye[0], eye[2]] };
        let off = |v: f32| ((v / 0.0625).round() as i32 + 2048).clamp(0, 4095) as u32;
        w[42] = (off(rest[0] - eye[0]) + 4096 * off(rest[1] - eye[2])) as f32;
        f.eye_under = level > 0 && inside && eye[1] < fr.sea.level + f.height_at([eye[0], eye[2]], 0.0);
        let code = storm_code(s) | FIELD_BIT | if f.eye_under { UNDER_BIT } else { 0 };
        w[27] = (level as u32 + 4 * code) as f32;
        w[30] = v_unres;
        // Spray and the breaking chance share a word, in steps of 0.01.
        let q = |v: f32, max: u32| ((v.max(0.0) / 0.01).round() as u32).min(max);
        // (Dev builds: the foam view's switch above them, bit 16.)
        w[31] = (q(s.spray, 255) + 256 * q(brk_chance(s), 255) + if s.foam_debug > 0.5 { FOAM_DEBUG_BIT } else { 0 }) as f32;
        w[38] = lam;
        w[39] = hi0;
        w[40] = lamf;
        f.words = w;
        f
    }

    /// The bands' sum at rest point `p` (relative to nothing: world x, z):
    /// sideways squeeze (as much of it as the mesh keeps: `squeeze_cap`),
    /// height, keeping the bands whose shortest wave is over `smooth` / 2 m.
    pub fn at_rest(&self, p: [f32; 2], smooth: f32) -> ([f32; 2], f32) {
        let q = [p[0] - self.centre[0], p[1] - self.centre[1]];
        let mut bd = [[0.0f32; 2]; BANDS];
        let mut bh = [0.0f32; BANDS];
        // (The long bands' tensor and the rest's: `SQUEEZE_LONG`.)
        let mut ml = [0.0f32; 3];
        let mut ms = [0.0f32; 3];
        for c in self.comps.iter().filter(|c| c.band < BANDS && band_hi(self.hi0, c.band) / RHO >= smooth * 0.5) {
            let th = c.k[0] * q[0] + c.k[1] * q[1] + c.ph;
            let (sn, cs) = th.sin_cos();
            let km = (c.k[0] * c.k[0] + c.k[1] * c.k[1]).sqrt();
            bh[c.band] += c.a * sn;
            bd[c.band][0] += self.lam * c.cf * c.k[0] / km * c.a * cs;
            bd[c.band][1] += self.lam * c.cf * c.k[1] / km * c.a * cs;
            let s = c.cf * c.a * sn / km;
            let m = if c.band < SQUEEZE_LONG { &mut ml } else { &mut ms };
            m[0] += c.k[0] * c.k[0] * s;
            m[1] += c.k[1] * c.k[1] * s;
            m[2] += c.k[0] * c.k[1] * s;
        }
        let (mut dl, mut ds, mut h) = ([0.0f32; 2], [0.0f32; 2], 0.0f32);
        for b in 0..BANDS {
            h += bh[b];
            let d = if b < SQUEEZE_LONG { &mut dl } else { &mut ds };
            d[0] += bd[b][0];
            d[1] += bd[b][1];
        }
        let ql = self.sea_q(self.depth) * self.lam;
        let sql = squeeze_cap([ql * ml[0], ql * ml[1], ql * ml[2]]);
        let sqs = squeeze_cap([0, 1, 2].map(|i| ql * (ml[i] * sql + ms[i])));
        ([dl[0] * sql + ds[0] * sqs, dl[1] * sql + ds[1] * sqs], h)
    }

    /// How much of a wave stands over depth `d` (the shader's `sea_q`, with
    /// its trough bound of 4 rms heights).
    fn sea_q(&self, d: f32) -> f32 {
        (0.45 * d / (4.0 * self.hrms).max(1e-3)).clamp(0.0, 1.0)
    }

    /// The surface's height over the level at world point `x` (the mesh
    /// moved the water there from its rest point: found by a few
    /// fixed-point steps), counting waves longer than about `smooth` m.
    /// Open sea (the beach's shrink is left out); 0 on Low.
    pub fn height_at(&self, x: [f32; 2], smooth: f32) -> f32 {
        if self.level == 0 {
            return 0.0;
        }
        let q = self.sea_q(self.depth);
        (q * self.at_rest(self.rest_under(x, smooth), smooth).1).max(0.15 - self.depth)
    }

    /// The rest point the waves' squeeze carries to world point `x`.
    pub fn rest_under(&self, x: [f32; 2], smooth: f32) -> [f32; 2] {
        let q = self.sea_q(self.depth);
        let mut p = x;
        for _ in 0..6 {
            let (d, _) = self.at_rest(p, smooth);
            p = [x[0] - q * d[0], x[1] - q * d[1]];
        }
        p
    }

    /// Where the mesh's centre (the shader's `fd_centre`) is carried to, less
    /// the eye (m): small, or the water in front of a low eye has no rings.
    pub fn centre_miss(&self, eye: [f32; 2]) -> f32 {
        let w = (self.words[42] + 0.5) as u32;
        let off = |v: u32| (v as f32 - 2048.0) * 0.0625;
        let c = [eye[0] + off(w & 4095), eye[1] + off(w >> 12)];
        let q = self.sea_q(self.depth);
        let d = self.at_rest(c, 0.0).0;
        ((c[0] + q * d[0] - eye[0]).powi(2) + (c[1] + q * d[1] - eye[1]).powi(2)).sqrt()
    }

    /// The deepest and the highest the waves stand over a `side` m square
    /// round `c`, sampled every `step` m at rest points (tests).
    pub fn extremes(&self, c: [f32; 2], side: f32, step: f32) -> (f32, f32) {
        let n = (side / step).ceil() as i32;
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        let q = self.sea_q(self.depth);
        for i in -n / 2..=n / 2 {
            for j in -n / 2..=n / 2 {
                let h = q * self.at_rest([c[0] + i as f32 * step, c[1] + j as f32 * step], 0.0).1;
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        (lo, hi)
    }

    /// The squeezed surface's folding at rest point `p` (the shader's
    /// Jacobian at the geometry's squeeze; 1 = none, under 0 folded over).
    pub fn folding(&self, p: [f32; 2]) -> f32 {
        let q = [p[0] - self.centre[0], p[1] - self.centre[1]];
        let mut m = [0.0f32; 3];
        for c in self.comps.iter().filter(|c| c.band < BANDS) {
            let th = c.k[0] * q[0] + c.k[1] * q[1] + c.ph;
            let km = (c.k[0] * c.k[0] + c.k[1] * c.k[1]).sqrt();
            let s = c.cf * c.a * th.sin() / km;
            m[0] += c.k[0] * c.k[0] * s;
            m[1] += c.k[1] * c.k[1] * s;
            m[2] += c.k[0] * c.k[1] * s;
        }
        let l = self.lam;
        (1.0 - l * m[0]) * (1.0 - l * m[1]) - l * l * m[2] * m[2]
    }
}

// ---- breaking crests (the shader's sea_field_break.wgsl) -------------------------

/// The cells and timing of a sea's breaking crests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Brk {
    pub lm: f32,
    pub cx: f32,
    pub cy: f32,
    pub c: f32,
    pub tf: f32,
    pub tc: f32,
    pub tr: f32,
    pub ev: f32,
    pub cyc: f32,
    pub nx: i32,
}

/// The shader's `brk_params` for peak wave number `kp`.
pub fn brk_params(kp: f32) -> Brk {
    let lm = (0.33 * TAU / kp.max(1e-5)).clamp(10.0, 130.0);
    let (cx, cy) = (0.6 * lm, 0.9 * lm);
    let c0 = (9.81 * lm / TAU).sqrt();
    let nx = (c0 * 300.0 / cx).round().max(1.0);
    let per = lm / c0;
    let (tf, tc, tr) = (0.5 * per, 0.3 * per, 0.35 * per);
    let ev = tf + tc + tr;
    Brk { lm, cx, cy, c: nx * cx / 300.0, tf, tc, tr, ev, cyc: 300.0 / (300.0 / (3.0 * ev)).round().max(1.0), nx: nx as i32 }
}

fn pcg(x: u32) -> u32 {
    let s = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

fn hash2(a: i32, b: i32) -> f32 {
    let h = pcg((a as u32) ^ pcg(b as u32));
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

fn hash3(a: i32, b: i32, c: i32) -> f32 {
    let h = pcg((a as u32) ^ pcg((b as u32) ^ pcg(c as u32)));
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// A breaking wave's shape (the shader's `brk_fold`): at a point `xf` m ahead of
/// the crest on a wave standing `env` m, strength `s`, moment `ev` (fold,
/// collapse, settle, window), breaking length `lm`: how far the point goes
/// downwind and up.
pub fn brk_fold(xf: f32, env: f32, s: f32, ev: [f32; 4], lm: f32, lean: f32) -> [f32; 2] {
    let l = (1.5 * env).min(0.12 * lm) * lean * s.powf(0.7) * ev[0] * (1.0 + 0.5 * ev[1]) * ev[2];
    let xa = 0.01 * lm;
    let w = (0.45 * l).max(0.02 * lm);
    let u1 = ((xf - xa) / w).clamp(0.0, 1.0);
    let drop = u1 * u1 * (3.0 - 2.0 * u1);
    let go = l * smoothstep(-0.2 * lm, 0.0, xf) * (1.0 - drop);
    let top = smoothstep(-0.6 * l - 1e-3, 0.0, xf - xa);
    let hollow = l * (0.2 * top + 0.55 * drop) * (1.0 - smoothstep(xa + w, xa + w + 0.12 * lm, xf));
    let (cr, cf) = (xf / (0.16 * lm), (xf - 0.06 * lm) / (0.2 * lm));
    let rise = 0.35 * env * s * ev[0] * (1.0 - ev[1]) * (-cr * cr).exp();
    let fall = 0.8 * env * s * ev[1] * lean * (-cf * cf).exp();
    [go, (rise - hollow - fall) * ev[2]]
}

impl Field {
    /// The chance a cell breaks in a cycle, as the shader reads it.
    pub fn brk_chance(&self) -> f32 {
        (((self.words[31].max(0.0) + 0.5) as u32 >> 8) & 255) as f32 * 0.01
    }

    /// A breaking event at world point `p` now (the shader's `brk_at`):
    /// fold, collapse, settle, the cell's window; all 0 with none.
    pub fn brk_at(&self, p: [f32; 2]) -> [f32; 4] {
        let chance = self.brk_chance();
        if chance <= 0.0 {
            return [0.0; 4];
        }
        let pr = brk_params(self.words[32]);
        let t = self.words[11] * 300.0;
        let q = [p[0] - self.centre[0], p[1] - self.centre[1]];
        let (cs, sn) = (self.words[34].cos(), self.words[34].sin());
        let (wx, wy) = (q[0] * cs + q[1] * sn, -q[0] * sn + q[1] * cs);
        let j = (wy / pr.cy).floor() as i32;
        let x = wx - pr.c * t + pr.cx * hash2(j, 7);
        let i = (x / pr.cx).floor() as i32;
        let n = (t / pr.cyc).floor() as i32;
        let ii = i.rem_euclid(pr.nx);
        let ncyc = ((300.0 / pr.cyc).round() as i32).max(1);
        let nn = n.rem_euclid(ncyc);
        if hash3(ii, j, nn) >= chance {
            return [0.0; 4];
        }
        let rel = t - n as f32 * pr.cyc - (pr.cyc - pr.ev) * hash3(ii + 1009, j, nn);
        if !(0.0..=pr.ev).contains(&rel) {
            return [0.0; 4];
        }
        let (fx, fy) = ((x / pr.cx).rem_euclid(1.0), (wy / pr.cy).rem_euclid(1.0));
        // (Without the shader's noise on its edge: within a fifth of a cell.)
        let rr = 2.0 * ((fx - 0.5) * (fx - 0.5) + (fy - 0.5) * (fy - 0.5)).sqrt();
        let win = 1.0 - smoothstep(0.45, 0.95, rr);
        [smoothstep(0.0, pr.tf, rel), smoothstep(pr.tf, pr.tf + pr.tc, rel), 1.0 - smoothstep(pr.tf + pr.tc, pr.ev, rel), win]
    }

    /// The wave about the breaking length at world point `p` (the shader's
    /// `brk_wave`, at the rest point): radians from its crest (ahead of it
    /// downwind positive), its height there (m), that height in those waves' rms.
    pub fn brk_wave(&self, p: [f32; 2]) -> [f32; 3] {
        let pr = brk_params(self.words[32]);
        let q = [p[0] - self.centre[0], p[1] - self.centre[1]];
        let r = [self.words[34].cos(), self.words[34].sin()];
        let (mut h, mut g, mut sg) = (0.0f32, 0.0f32, 0.0f32);
        let keep = |b: usize| {
            let hi = band_hi(self.hi0, b);
            !(hi / RHO > 1.5 * pr.lm || hi < 0.5 * pr.lm)
        };
        for b in (0..BANDS).filter(|&b| keep(b)) {
            sg += self.brms[b] * self.brms[b];
        }
        for c in self.comps.iter().filter(|c| c.band < BANDS && keep(c.band)) {
            let hi = band_hi(self.hi0, c.band);
            let (sn, cs) = (c.k[0] * q[0] + c.k[1] * q[1] + c.ph).sin_cos();
            h += c.a * sn;
            g += c.a * cs * (c.k[0] * r[0] + c.k[1] * r[1]) * hi / RHO.sqrt() / TAU;
        }
        let env = (h * h + g * g).sqrt();
        [(-g).atan2(h), env, env / sg.sqrt().max(1e-3)]
    }
}
