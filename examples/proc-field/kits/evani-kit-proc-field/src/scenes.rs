//! Evani.commons' test scenes and measures (kit feature `commons`, the
//! render app only; docs/EVANI_COMMONS.md "Test scenes and measures"). A
//! behaviour asset (catalog/procedural/ref_classes.json class `behaviour`)
//! is judged by its effect on a standard scene, staged here around it:
//!
//! 1. wind-field: a 20 m meadow of knee-high grass, two young 5 m trees and
//!    a flag on a 3 m pole, moved by the asset's wind;
//! 2. current-channel: a 4 m wide stream between grassy banks, one rock
//!    mid-channel and floating leaves, carried by the asset's water flow;
//! 3. pose-set and 4. expression-set: the asset itself, one pose per frame
//!    (`kit.proc_field.pose` 0..3).
//!
//! The proxies' physics is the scene's own and fixed; the asset only gives
//! the field (evani-procedural's `behaviour`) or its own pose params. The
//! probes measure the proxies' (or the generated mesh's) positions, never a
//! value the asset reports about itself. Everything is a pure function of
//! the sim step (1/60 s), so a frame at a given `time_s` is always the same.

use evani_procedural::behaviour::Channel;

/// A wind field as the scene reads it: velocity at a point and time.
pub type WindField<'a> = dyn Fn([f32; 3], f32) -> [f32; 3] + 'a;
/// A water surface flow as the scene reads it.
pub type FlowField<'a> = dyn Fn(&Channel, [f32; 3], f32) -> [f32; 2] + 'a;
/// An asset's meshes: each part's vertices and indices.
pub type MeshSet = Vec<(Vec<[f32; 6]>, Vec<u32>)>;

/// The sim step (s).
pub const DT: f32 = 1.0 / 60.0;

/// The test scenes by `kit.proc_field.test_scene` value (ref_classes.json `test_scenes`).
pub const SCENES: [&str; 4] = ["wind-field", "current-channel", "pose-set", "expression-set"];

/// The spec ids each scene measures, in ref_classes.json's order.
pub const WIND_IDS: [&str; 11] = [
    "grass_sway_amp", "grass_sway_hz", "gust_peak_ratio", "gust_interval_s", "gust_interval_cv", "gust_front_speed",
    "direction_coherence", "tree_crown_hz", "tree_crown_amp", "flag_flutter_hz", "flag_alignment_deg",
];
pub const CURRENT_IDS: [&str; 4] = ["surface_speed", "bank_ratio", "direction_follow_deg", "eddy_period_s"];
pub const POSE_IDS: [&str; 5] = ["elbow_range_deg", "knee_range_deg", "volume_kept", "intersection_cm", "foot_slide_cm"];
pub const EXPRESSION_IDS: [&str; 4] = ["symmetry_err", "blink_gap_mm", "lip_seal_mm", "teeth_clip_mm"];

/// One measured line: spec id and value (`None`: the probes could not
/// measure it for this asset, printed as `null`).
pub type Measures = Vec<(&'static str, Option<f32>)>;

/// `evani-field-measure {"<id>": number|null, ...}`.
pub fn measure_line(m: &Measures) -> String {
    let body: Vec<String> = m
        .iter()
        .map(|(k, v)| match v {
            Some(x) if x.is_finite() => format!("\"{k}\":{}", round4(*x)),
            _ => format!("\"{k}\":null"),
        })
        .collect();
    format!("evani-field-measure {{{}}}", body.join(","))
}

/// Four significant decimals (stable text across platforms).
fn round4(x: f32) -> f64 {
    (f64::from(x) * 10_000.0).round() / 10_000.0
}

// ---------------------------------------------------------------- wind-field

/// The meadow: 20 m square round the origin, the camera on its +z edge.
pub const MEADOW_HALF: f32 = 10.0;
/// Grass: knee-high blades, natural sway 1.2 Hz, damping 0.25.
pub const GRASS_H: f32 = 0.5;
const GRASS_F0: f32 = 1.2;
const GRASS_ZETA: f32 = 0.25;
/// Tip offset per (m/s)^1.2 of wind (flexible blades bend less than
/// quadratically: Vogel's exponent).
const GRASS_C: f32 = 0.016;
/// The trees: 5 m tall, crown sway 0.35 Hz, damping 0.15.
pub const TREE_H: f32 = 5.0;
const TREE_F0: f32 = 0.35;
const TREE_ZETA: f32 = 0.15;
const TREE_C: f32 = 0.0256;
pub const TREES: [[f32; 2]; 2] = [[-4.0, -3.0], [5.0, -6.0]];
/// The flag: a 1 m by 0.6 m flag on a 3 m pole.
pub const FLAG_POLE: [f32; 2] = [1.5, -1.0];
pub const POLE_H: f32 = 3.0;
pub const FLAG_L: f32 = 1.0;
/// The flag turns to the wind with this lag (s) and flaps at
/// `FLAG_ST * speed / FLAG_L` Hz.
const FLAG_TAU: f32 = 0.4;
const FLAG_ST: f32 = 0.75;
const FLAG_AMP: f32 = 0.12;

/// A damped spring (2D, horizontal) pulled towards `c * |u|^1.2` along the
/// wind: one blade tip or one tree crown.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sway {
    pub d: [f32; 2],
    v: [f32; 2],
}

impl Sway {
    fn step(&mut self, u: [f32; 3], f0: f32, zeta: f32, c: f32, cap: f32) {
        let s = (u[0] * u[0] + u[2] * u[2]).sqrt();
        let k = if s > 1e-6 { c * s.powf(0.2) } else { 0.0 };
        let mut tg = [u[0] * k, u[2] * k];
        let m = (tg[0] * tg[0] + tg[1] * tg[1]).sqrt();
        if m > cap {
            tg = [tg[0] * cap / m, tg[1] * cap / m];
        }
        let w = std::f32::consts::TAU * f0;
        for ((v, d), t) in self.v.iter_mut().zip(self.d.iter_mut()).zip(tg) {
            *v += (w * w * (t - *d) - 2.0 * zeta * w * *v) * DT;
            *d += *v * DT;
        }
    }
}

/// A blade's tip offset at step `n`: simulated from rest 120 steps (2 s,
/// over four of its damping times) before, so it depends on `n` alone.
pub fn grass_at(wind: &WindField<'_>, p: [f32; 2], n: u64) -> [f32; 2] {
    let mut s = Sway::default();
    for k in n.saturating_sub(120)..n {
        s.step(wind([p[0], GRASS_H, p[1]], k as f32 * DT), GRASS_F0, GRASS_ZETA, GRASS_C, GRASS_H * 0.7);
    }
    s.d
}

/// A tree crown's offset at step `n` (15 s from rest before).
pub fn tree_at(wind: &WindField<'_>, p: [f32; 2], n: u64) -> [f32; 2] {
    let mut s = Sway::default();
    for k in n.saturating_sub(900)..n {
        s.step(wind([p[0], TREE_H * 0.8, p[1]], k as f32 * DT), TREE_F0, TREE_ZETA, TREE_C, TREE_H * 0.2);
    }
    s.d
}

/// The flag's state: the line's azimuth, how far it streams out (0 hanging,
/// 1 straight) and its flap phase.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Flag {
    pub azimuth: f32,
    pub extend: f32,
    pub phase: f32,
}

impl Flag {
    fn step(&mut self, u: [f32; 3], first: bool) {
        let s = (u[0] * u[0] + u[2] * u[2]).sqrt();
        let az = u[2].atan2(u[0]);
        if first {
            self.azimuth = az;
        }
        if s > 0.05 {
            let mut d = az - self.azimuth;
            d = (d + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            self.azimuth += d * (DT / FLAG_TAU).min(1.0);
        }
        let ext = (s / 2.5).clamp(0.0, 1.0);
        self.extend += (ext - self.extend) * (DT / FLAG_TAU).min(1.0);
        self.phase = (self.phase + std::f32::consts::TAU * FLAG_ST * s / FLAG_L * DT).rem_euclid(std::f32::consts::TAU);
    }

    /// The free edge's middle (world) for a pole at `p`.
    pub fn edge(&self, p: [f32; 2]) -> [f32; 3] {
        let e = self.extend.max(0.1);
        let (sa, ca) = self.azimuth.sin_cos();
        let lat = FLAG_AMP * e * self.phase.sin();
        let reach = FLAG_L * e;
        [p[0] + ca * reach - sa * lat, POLE_H - 0.3 - FLAG_L * 0.8 * (1.0 - e), p[1] + sa * reach + ca * lat]
    }

    /// The edge's offset across the flag's line (m): its flutter.
    pub fn lateral(&self) -> f32 {
        FLAG_AMP * self.extend.max(0.1) * self.phase.sin()
    }
}

/// The flag at step `n` (its phase needs the whole run from 0).
pub fn flag_at(wind: &WindField<'_>, n: u64) -> Flag {
    let mut f = Flag::default();
    for k in 0..n.min(36_000) {
        f.step(wind([FLAG_POLE[0], POLE_H - 0.3, FLAG_POLE[1]], k as f32 * DT), k == 0);
    }
    f
}

/// The probe blades: a 6 x 6 grid 3 m apart over the meadow.
pub fn probes() -> Vec<[f32; 2]> {
    (0..36).map(|i| [-7.5 + 3.0 * (i % 6) as f32, -7.5 + 3.0 * (i / 6) as f32]).collect()
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    v.retain(|x| x.is_finite());
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { 0.5 * (v[n / 2 - 1] + v[n / 2]) })
}

fn mean(v: &[f32]) -> Option<f32> {
    (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
}

/// The strongest frequency (Hz) of `x` sampled every `dt` s, searched from
/// `lo` to `hi` in steps of `df` (a plain DFT, mean removed); `None` for a
/// signal that hardly moves.
pub fn dominant_hz(x: &[f32], dt: f32, lo: f32, hi: f32, df: f32) -> Option<f32> {
    let m = mean(x)?;
    let var = x.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / x.len() as f32;
    if var < 1e-10 {
        return None;
    }
    let (mut best, mut bf) = (0.0f32, None);
    let mut f = lo;
    while f <= hi + 1e-6 {
        let w = std::f32::consts::TAU * f * dt;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, v) in x.iter().enumerate() {
            let (s, c) = (w * i as f32).sin_cos();
            re += (v - m) * c;
            im += (v - m) * s;
        }
        let p = re * re + im * im;
        if p > best {
            best = p;
            bf = Some(f);
        }
        f += df;
    }
    bf
}

fn len2(a: [f32; 2]) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

/// The wind-field's measures: 65 s of sim (the first 5 s settle), every
/// probe blade, both trees and the flag.
pub fn wind_measures(wind: &WindField<'_>) -> Measures {
    const WARM: u64 = 300;
    const N: u64 = 3600;
    let pr = probes();
    let mut blades = vec![Sway::default(); pr.len()];
    let mut trees = [Sway::default(); 2];
    let mut flag = Flag::default();
    let mut tips: Vec<Vec<[f32; 2]>> = vec![Vec::with_capacity(N as usize); pr.len()];
    let mut crowns: [Vec<[f32; 2]>; 2] = [Vec::new(), Vec::new()];
    let (mut lat, mut align) = (Vec::new(), Vec::new());
    for k in 0..WARM + N {
        let t = k as f32 * DT;
        for (b, p) in blades.iter_mut().zip(&pr) {
            b.step(wind([p[0], GRASS_H, p[1]], t), GRASS_F0, GRASS_ZETA, GRASS_C, GRASS_H * 0.7);
        }
        for (s, p) in trees.iter_mut().zip(TREES) {
            s.step(wind([p[0], TREE_H * 0.8, p[1]], t), TREE_F0, TREE_ZETA, TREE_C, TREE_H * 0.2);
        }
        let uf = wind([FLAG_POLE[0], POLE_H - 0.3, FLAG_POLE[1]], t);
        flag.step(uf, k == 0);
        if k < WARM {
            continue;
        }
        for (v, b) in tips.iter_mut().zip(&blades) {
            v.push(b.d);
        }
        for (v, s) in crowns.iter_mut().zip(&trees) {
            v.push(s.d);
        }
        lat.push(flag.lateral());
        // The flag's line (pole top to the free edge's middle, measured on
        // its geometry) against the wind at the pole.
        let e = flag.edge(FLAG_POLE);
        let line = [e[0] - FLAG_POLE[0], e[2] - FLAG_POLE[1]];
        let s = (uf[0] * uf[0] + uf[2] * uf[2]).sqrt();
        if s > 0.3 && len2(line) > 1e-4 {
            let d = line[1].atan2(line[0]) - uf[2].atan2(uf[0]);
            let d = (d + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            align.push(d.abs().to_degrees());
        }
    }
    let n = N as usize;
    // The mean lean direction over the field.
    let mut md = [0.0f32; 2];
    for v in &tips {
        for d in v {
            md = [md[0] + d[0], md[1] + d[1]];
        }
    }
    let ml = len2(md);
    let dir = if ml > 1e-9 { [md[0] / ml, md[1] / ml] } else { [1.0, 0.0] };
    let along = |d: &[f32; 2]| d[0] * dir[0] + d[1] * dir[1];
    // Sway amplitude and frequency: per 10 s window and blade.
    let (mut amps, mut hzs) = (Vec::new(), Vec::new());
    for w in 0..6 {
        let r = w * 600..(w + 1) * 600;
        let mut a = Vec::new();
        for v in &tips {
            let s: Vec<f32> = v[r.clone()].iter().map(along).collect();
            let (lo, hi) = s.iter().fold((f32::MAX, f32::MIN), |(l, h), x| (l.min(*x), h.max(*x)));
            a.push(hi - lo);
            // The sway about the blade's slow lean (its 1 s moving mean
            // taken out), so a gust's swell doesn't read as the sway.
            let sub: Vec<f32> = (0..s.len()).step_by(2).map(|i| {
                let (a, b) = (i.saturating_sub(30), (i + 30).min(s.len() - 1));
                s[i] - s[a..=b].iter().sum::<f32>() / (b - a + 1) as f32
            }).collect();
            if let Some(f) = dominant_hz(&sub, DT * 2.0, 0.2, 5.0, 0.1) {
                hzs.push(f);
            }
        }
        amps.push(mean(&a).unwrap_or(0.0));
    }
    // The field's sway, smoothed over 0.5 s, and its gusts.
    let raw: Vec<f32> = (0..n).map(|i| tips.iter().map(|v| len2(v[i])).sum::<f32>() / tips.len() as f32).collect();
    let sm: Vec<f32> = (0..n).map(|i| {
        let (a, b) = (i.saturating_sub(15), (i + 15).min(n - 1));
        raw[a..=b].iter().sum::<f32>() / (b - a + 1) as f32
    }).collect();
    let calm = median(sm.clone()).unwrap_or(0.0);
    let mut gusts: Vec<(usize, usize, f32)> = Vec::new();
    if calm > 1e-5 {
        let mut open: Option<(usize, f32)> = None;
        for (i, s) in sm.iter().enumerate() {
            match open {
                None if *s > 1.35 * calm => open = Some((i, *s)),
                Some((st, pk)) if *s < 1.15 * calm => {
                    gusts.push((st, i, pk));
                    open = None;
                }
                Some((st, pk)) => open = Some((st, pk.max(*s))),
                None => {}
            }
        }
        if let Some((st, pk)) = open {
            gusts.push((st, n - 1, pk));
        }
    }
    // Gust timing and strength where a gust is felt: at each probe blade
    // (its sway smoothed over 0.5 s, over 1.35 x its own median), pooled.
    let (mut iv, mut cvs, mut peaks) = (Vec::new(), Vec::new(), Vec::new());
    for v in &tips {
        let s: Vec<f32> = (0..n).map(|i| {
            let (a, b) = (i.saturating_sub(15), (i + 15).min(n - 1));
            v[a..=b].iter().map(|d| len2(*d)).sum::<f32>() / (b - a + 1) as f32
        }).collect();
        let c = median(s.clone()).unwrap_or(0.0);
        if c <= 1e-5 {
            continue;
        }
        let (mut starts, mut pk, mut open) = (Vec::new(), 0.0f32, false);
        for (i, x) in s.iter().enumerate() {
            if !open && *x > 1.35 * c {
                open = true;
                starts.push(i);
            } else if open && *x < 1.15 * c {
                open = false;
            }
            if open {
                pk = pk.max(*x);
            }
        }
        if !starts.is_empty() {
            peaks.push(pk / c);
        }
        let b: Vec<f32> = starts.windows(2).map(|w| (w[1] - w[0]) as f32 * DT).collect();
        if b.len() >= 3 {
            let m = mean(&b).unwrap_or(0.0).max(1e-6);
            cvs.push((b.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / b.len() as f32).sqrt() / m);
        }
        iv.extend(b);
    }
    let peak = median(peaks);
    let iv_mean = mean(&iv);
    let cv = median(cvs);
    // The front's speed: the time the bend wave takes between two probe
    // blades 6 to 15 m apart along the wind (and within 2 m across it):
    // the lag that best lines up their sway (smoothed over 1 s, its mean
    // taken out), from 0.1 s up to a 2 m/s crossing.
    let slow: Vec<Vec<f32>> = tips.iter().map(|v| {
        let s: Vec<f32> = (0..n).step_by(2).map(|i| {
            let (a, b) = (i.saturating_sub(30), (i + 30).min(n - 1));
            v[a..=b].iter().map(|d| len2(*d)).sum::<f32>() / (b - a + 1) as f32
        }).collect();
        let m = mean(&s).unwrap_or(0.0);
        s.into_iter().map(|x| x - m).collect()
    }).collect();
    let mut speeds = Vec::new();
    for (a, pa) in pr.iter().enumerate() {
        for (b, pb) in pr.iter().enumerate() {
            let rel = [pb[0] - pa[0], pb[1] - pa[1]];
            let (al, cr) = (rel[0] * dir[0] + rel[1] * dir[1], (rel[1] * dir[0] - rel[0] * dir[1]).abs());
            if !(6.0..=15.0).contains(&al) || cr > 2.0 {
                continue;
            }
            let (sa, sb) = (&slow[a], &slow[b]);
            let e = (sa.iter().map(|x| x * x).sum::<f32>() * sb.iter().map(|x| x * x).sum::<f32>()).sqrt();
            if e < 1e-12 {
                continue;
            }
            let max_lag = ((al / 2.0) / (DT * 2.0)) as usize;
            let (mut best, mut lag) = (0.3f32, None);
            for l in 3..=max_lag.min(sa.len() / 2) {
                let c = sa.iter().zip(&sb[l..]).map(|(x, y)| x * y).sum::<f32>() / e;
                if c > best {
                    (best, lag) = (c, Some(l));
                }
            }
            if let Some(l) = lag {
                speeds.push(al / (l as f32 * DT * 2.0));
            }
        }
    }
    // Direction coherence: the mean cosine between the lean of probe
    // blades up to 5 m apart, every 0.25 s.
    let mut cos = Vec::new();
    for (a, pa) in pr.iter().enumerate() {
        for (b, pb) in pr.iter().enumerate().skip(a + 1) {
            if len2([pa[0] - pb[0], pa[1] - pb[1]]) > 5.0 {
                continue;
            }
            for i in (0..n).step_by(15) {
                let (da, db) = (tips[a][i], tips[b][i]);
                let (la, lb) = (len2(da), len2(db));
                if la > 1e-4 && lb > 1e-4 {
                    cos.push((da[0] * db[0] + da[1] * db[1]) / (la * lb));
                }
            }
        }
    }
    // The trees and the flag.
    let mut crown_hz = Vec::new();
    let mut crown_amp = Vec::new();
    for v in &crowns {
        let mut cd = v.iter().fold([0.0f32; 2], |a, d| [a[0] + d[0], a[1] + d[1]]);
        let cl = len2(cd);
        cd = if cl > 1e-9 { [cd[0] / cl, cd[1] / cl] } else { [1.0, 0.0] };
        let s: Vec<f32> = v.iter().step_by(2).map(|d| d[0] * cd[0] + d[1] * cd[1]).collect();
        if let Some(f) = dominant_hz(&s, DT * 2.0, 0.05, 3.0, 0.01) {
            crown_hz.push(f);
        }
        crown_amp.push(v.iter().map(|d| len2(*d)).fold(0.0, f32::max) / TREE_H);
    }
    let mut flag_hz = Vec::new();
    for w in 0..6 {
        if let Some(f) = dominant_hz(&lat[w * 600..(w + 1) * 600], DT, 0.2, 10.0, 0.1) {
            flag_hz.push(f);
        }
    }
    let vals = [
        median(amps),
        median(hzs),
        peak,
        iv_mean,
        cv,
        median(speeds),
        mean(&cos),
        mean(&crown_hz),
        mean(&crown_amp),
        median(flag_hz),
        mean(&align),
    ];
    WIND_IDS.iter().copied().zip(vals).collect()
}

// ----------------------------------------------------------- current-channel

/// The stream: along +x from x = -8 to 8, 4 m wide, its rock mid-channel.
pub const CHANNEL_HALF_LEN: f32 = 8.0;
pub const ROCK: ([f32; 2], f32) = ([3.5, 0.0], 0.35);

pub fn channel() -> Channel {
    Channel { half_width: 2.0, rocks: vec![ROCK] }
}

/// One floating marker moved by the flow (midpoint steps, kept off the
/// banks and out of the rocks).
pub fn drift(flow: &FlowField<'_>, c: &Channel, p: [f32; 2], t: f32) -> [f32; 2] {
    let u = flow(c, [p[0], 0.0, p[1]], t);
    let m = [p[0] + u[0] * DT * 0.5, p[1] + u[1] * DT * 0.5];
    let u = flow(c, [m[0], 0.0, m[1]], t + DT * 0.5);
    let mut q = [p[0] + u[0] * DT, (p[1] + u[1] * DT).clamp(-c.half_width + 0.05, c.half_width - 0.05)];
    for &([rx, rz], r) in &c.rocks {
        let d = [q[0] - rx, q[1] - rz];
        let l = len2(d);
        if l < r + 0.02 && l > 1e-6 {
            q = [rx + d[0] / l * (r + 0.02), rz + d[1] / l * (r + 0.02)];
        }
    }
    q
}

/// A path from `p` released at step `n0`, `steps` long (every step's point).
fn path(flow: &FlowField<'_>, c: &Channel, p: [f32; 2], n0: u64, steps: u64) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(steps as usize + 1);
    let mut q = p;
    out.push(q);
    for k in n0..n0 + steps {
        q = drift(flow, c, q, k as f32 * DT);
        out.push(q);
    }
    out
}

/// The current-channel's measures (released at t = 2 s).
pub fn current_measures(flow: &FlowField<'_>) -> Measures {
    let c = channel();
    let n0 = 120;
    let speed = |z: f32| {
        let p = path(flow, &c, [-7.0, z], n0, 300);
        let (a, b) = (p[0], p[p.len() - 1]);
        len2([b[0] - a[0], b[1] - a[1]]) / 5.0
    };
    let mid: Vec<f32> = [-0.3f32, 0.0, 0.3].into_iter().map(speed).collect();
    let mid = mean(&mid).unwrap_or(0.0);
    let bank = mean(&[speed(1.5), speed(-1.5)]).unwrap_or(0.0);
    // Paths across the channel, sampled every 0.5 s, away from the rocks.
    let mut ang = Vec::new();
    for z in [-1.5f32, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5] {
        let p = path(flow, &c, [-7.0, z], n0, 720);
        for w in p.iter().step_by(30).collect::<Vec<_>>().windows(2) {
            let (a, b) = (*w[0], *w[1]);
            let d = [b[0] - a[0], b[1] - a[1]];
            let near = c.rocks.iter().any(|&([rx, rz], r)| len2([a[0] - rx, a[1] - rz]) < r + 1.5);
            if near || a[0] > CHANNEL_HALF_LEN - 0.5 || len2(d) < 0.01 {
                continue;
            }
            ang.push(d[1].atan2(d[0]).abs().to_degrees());
        }
    }
    // The eddy: markers released in the rock's lee; one caught turns its
    // heading a full circle. Its period: the time that takes.
    let ([rx, rz], r) = ROCK;
    let mut periods = Vec::new();
    for i in 0..6 {
        for j in 0..5 {
            let p0 = [rx + r + 0.15 + 0.45 * i as f32, rz - 0.4 + 0.2 * j as f32];
            let p = path(flow, &c, p0, n0, 1800);
            let mut turn = 0.0f32;
            let mut prev: Option<f32> = None;
            for (k, w) in p.windows(2).enumerate() {
                let d = [w[1][0] - w[0][0], w[1][1] - w[0][1]];
                if len2(d) < 1e-6 {
                    continue;
                }
                let h = d[1].atan2(d[0]);
                if let Some(q) = prev {
                    turn += (h - q + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                }
                prev = Some(h);
                if turn.abs() >= std::f32::consts::TAU {
                    periods.push((k + 1) as f32 * DT);
                    break;
                }
            }
        }
    }
    let vals = [
        (mid > 0.0).then_some(mid),
        (mid > 1e-3).then(|| bank / mid),
        mean(&ang),
        median(periods),
    ];
    CURRENT_IDS.iter().copied().zip(vals).collect()
}

/// The floating leaves at step `n`: each drifts 14 s from its release
/// point, then starts again (staggered), so the frames always show some.
pub fn leaves_at(flow: &FlowField<'_>, n: u64) -> Vec<[f32; 2]> {
    const LIFE: u64 = 840;
    let c = channel();
    let ([rx, rz], r) = ROCK;
    (0..19u64)
        .map(|i| {
            let start = if i < 16 {
                [-7.6 + 0.35 * (i % 4) as f32, -1.7 + 3.4 * (((i * 7) % 16) as f32 + 0.5) / 16.0]
            } else {
                [rx + r + 0.4 + 0.2 * (i - 16) as f32, rz + 0.25 - 0.2 * (i - 16) as f32]
            };
            let age = (n + i * LIFE / 16) % LIFE;
            let p = path(flow, &c, start, n - age.min(n), age.min(n));
            p[p.len() - 1]
        })
        .collect()
}

// ------------------------------------------------------ pose and expression

/// The params each pose sets when the asset has no `pose` param of its
/// own (name, value per pose 0..3); a name the asset lacks is skipped.
pub const POSE_TABLE: [(&str, [f32; 4]); 2] = [
    // rest, a walking stride, a deep crouch, reaching overhead
    ("arms", [10.0, 25.0, 35.0, 180.0]),
    ("knees", [0.0, 30.0, 110.0, 0.0]),
];
pub const EXPRESSION_TABLE: [(&str, [f32; 4]); 4] = [
    // neutral, a broad smile, surprise with the jaw open, a blink
    ("smile", [0.0, 1.0, 0.0, 0.0]),
    ("open", [0.0, 0.0, 1.0, 0.0]),
    ("brows", [0.0, 0.0, 1.0, 0.0]),
    ("blink", [0.0, 0.0, 0.0, 1.0]),
];

/// A closed mesh's enclosed volume (m^3, the divergence theorem).
pub fn volume(vertices: &[[f32; 6]], indices: &[u32]) -> f32 {
    let mut v = 0.0f64;
    for t in indices.as_chunks::<3>().0 {
        let p = |i: u32| vertices.get(i as usize).map(|v| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]).unwrap_or([0.0; 3]);
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        v += (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0;
    }
    v.abs() as f32
}

/// The pose-set's measures from the asset's meshes at poses 0..3 (each
/// pose: its parts' vertices and indices). Only what the meshes alone
/// show: the joints' ranges, skin intersections and foot slide need the
/// rig's joints, which a mesh doesn't name (`null`).
pub fn pose_measures(poses: &[MeshSet]) -> Measures {
    let vol = |p: &MeshSet| p.iter().map(|(v, i)| volume(v, i)).sum::<f32>();
    let kept = poses.first().map(vol).filter(|v| *v > 1e-9).map(|rest| poses.iter().skip(1).map(|p| vol(p) / rest).fold(f32::MAX, f32::min)).filter(|v| *v < f32::MAX);
    POSE_IDS.iter().copied().zip([None, None, kept, None, None]).collect()
}

/// The expression-set's measures from the asset's meshes at expressions
/// 0..3 (neutral first): the left-right symmetry of each expression's
/// vertex offsets from neutral, over the face's width. The lid, lip and
/// teeth gaps need parts a mesh doesn't name (`null`).
pub fn expression_measures(frames: &[MeshSet]) -> Measures {
    let sym = frames.first().and_then(|neutral| {
        let (lo, hi) = neutral.iter().flat_map(|(v, _)| v.iter()).fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(v[0]), h.max(v[0])));
        let width = hi - lo;
        if width <= 1e-6 {
            return None;
        }
        let mut worst: Option<f32> = None;
        for f in frames.iter().skip(1) {
            let (mut sum, mut cnt) = (0.0f32, 0usize);
            for ((nv, _), (fv, _)) in neutral.iter().zip(f) {
                if nv.len() != fv.len() {
                    continue;
                }
                let off: Vec<[f32; 3]> = nv.iter().zip(fv).map(|(a, b)| [b[0] - a[0], b[1] - a[1], b[2] - a[2]]).collect();
                // The mirror partner: the nearest neutral vertex to the
                // mirrored one, looked up in a grid of 1% of the width.
                let cell = width * 0.01;
                let key = |p: [f32; 3]| [(p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32, (p[2] / cell).floor() as i32];
                let mut grid: std::collections::HashMap<[i32; 3], Vec<usize>> = std::collections::HashMap::new();
                for (j, b) in nv.iter().enumerate() {
                    grid.entry(key([b[0], b[1], b[2]])).or_default().push(j);
                }
                for (i, a) in nv.iter().enumerate() {
                    let m = [-a[0], a[1], a[2]];
                    let km = key(m);
                    let (mut bj, mut bd) = (0usize, f32::MAX);
                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            for dz in -1..=1 {
                                for &j in grid.get(&[km[0] + dx, km[1] + dy, km[2] + dz]).map(Vec::as_slice).unwrap_or(&[]) {
                                    let b = nv[j];
                                    let d = (b[0] - m[0]).powi(2) + (b[1] - m[1]).powi(2) + (b[2] - m[2]).powi(2);
                                    if d < bd {
                                        (bj, bd) = (j, d);
                                    }
                                }
                            }
                        }
                    }
                    let (oi, oj) = (off[i], off[bj]);
                    let moving = oi.iter().chain(oj.iter()).any(|x| x.abs() > 1e-6);
                    if bd.sqrt() > width * 0.01 || !moving {
                        continue;
                    }
                    let e = [oi[0] + oj[0], oi[1] - oj[1], oi[2] - oj[2]];
                    sum += (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
                    cnt += 1;
                }
            }
            let v = if cnt > 0 { sum / cnt as f32 / width } else { 0.0 };
            worst = Some(worst.map_or(v, |w: f32| w.max(v)));
        }
        worst
    });
    EXPRESSION_IDS.iter().copied().zip([sym, None, None, None]).collect()
}
