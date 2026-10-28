//! Animation clip baking (STAGE3_PLAN S3-7): the master's glTF animations
//! become the compact clip block (`evani_guide::clipblock`, docs/MODEL_FORMAT.md
//! "Clips") that a tier file carries in its binary chunk.
//!
//! Per clip:
//! 1. Every channel is resampled at 30 Hz (STEP, LINEAR with slerp for
//!    rotations, CUBICSPLINE as Hermite curves).
//! 2. Error is measured where it shows: at virtual shell points around each
//!    joint (six points at the shell distance: the farthest direct child, the
//!    mesh radius of a rigid mesh node, never under 3 cm), in world space.
//! 3. For a per-track tolerance tau, a track that stays within tau of the
//!    rest pose is dropped, one within tau of its first frame is stored once
//!    (a constant), and any other keeps only the keys a greedy forward pass
//!    needs, measured on the quantised values the decoder will see.
//! 4. The clip is written, read back with the reference reader, and every
//!    frame of every joint below an animated one is compared with the
//!    resampled source at the shell points. The smallest clip within the
//!    error bound wins, over rotations in 32 bits (10-10-10-2) and in
//!    48 bits (smallest-three 3 x 15) and tau = bound / 2^k.
//!
//! Plain Rust written for Evani.Dev; deterministic (no clocks, no hash maps).

use std::collections::BTreeMap;

use evani_gltf::{AnimSampler, Gltf};
use evani_guide::clipblock::{self, ClipIn, ROTATION, SCALE, TRANSLATION, TrackIn};

/// The smallest shell distance (metres): a short joint still moves skin.
pub const MIN_SHELL_M: f64 = 0.03;
/// tau = bound / 2^k for k below this, then tau = 0.
const STEPS: i32 = 12;
/// Below this an error is rounding, not a change.
const EPS: f64 = 1e-7;

type M4 = [f64; 16];
type V4 = [f64; 4];

fn ident() -> M4 {
    let mut m = [0.0; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m
}

/// Column-major product a * b.
fn mul(a: &M4, b: &M4) -> M4 {
    let mut r = [0.0; 16];
    for c in 0..4 {
        for row in 0..4 {
            r[c * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[c * 4 + k]).sum();
        }
    }
    r
}

fn compose(t: &V4, q: &V4, s: &V4) -> M4 {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let [x, y, z, w] = if l > 0.0 { q.map(|c| c / l) } else { [0.0, 0.0, 0.0, 1.0] };
    [
        (1.0 - 2.0 * (y * y + z * z)) * s[0],
        2.0 * (x * y + z * w) * s[0],
        2.0 * (x * z - y * w) * s[0],
        0.0,
        2.0 * (x * y - z * w) * s[1],
        (1.0 - 2.0 * (x * x + z * z)) * s[1],
        2.0 * (y * z + x * w) * s[1],
        0.0,
        2.0 * (x * z + y * w) * s[2],
        2.0 * (y * z - x * w) * s[2],
        (1.0 - 2.0 * (x * x + y * y)) * s[2],
        0.0,
        t[0],
        t[1],
        t[2],
        1.0,
    ]
}

fn point(m: &M4, p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The largest axis scale of a matrix.
fn max_scale(m: &M4) -> f64 {
    (0..3).map(|c| (m[c * 4].powi(2) + m[c * 4 + 1].powi(2) + m[c * 4 + 2].powi(2)).sqrt()).fold(0.0, f64::max)
}

fn f64s<const N: usize>(a: [f32; N]) -> [f64; N] {
    a.map(f64::from)
}

fn v4(a: [f32; 4]) -> V4 {
    f64s(a)
}

fn channel_name(c: u8) -> &'static str {
    match c {
        TRANSLATION => "translation",
        ROTATION => "rotation",
        _ => "scale",
    }
}

/// The rest pose and where to measure error.
struct Rig {
    names: Vec<String>,
    parent: Vec<Option<usize>>,
    /// Parents first.
    order: Vec<usize>,
    /// Rest TRS (translation, rotation, scale) per node.
    rest: Vec<[V4; 3]>,
    /// Nodes with a `matrix` (never animated).
    matrix: Vec<Option<M4>>,
    /// Shell distance in metres (world) and in the node's own units.
    shell_w: Vec<f64>,
    shell_l: Vec<f64>,
    /// The parent's world scale at rest (1 for a root).
    parent_scale: Vec<f64>,
}

impl Rig {
    fn new(g: &Gltf) -> Rig {
        let n = g.nodes.len();
        let parent: Vec<Option<usize>> = (0..n).map(|i| g.parent(i)).collect();
        let mut order: Vec<usize> = (0..n).filter(|&i| parent[i].is_none()).collect();
        let mut at = 0;
        while at < order.len() {
            order.extend(g.nodes[order[at]].children.iter().copied());
            at += 1;
        }
        let rest: Vec<[V4; 3]> = g.nodes.iter().map(|x| [[f64::from(x.translation[0]), f64::from(x.translation[1]), f64::from(x.translation[2]), 0.0], v4(x.rotation), [f64::from(x.scale[0]), f64::from(x.scale[1]), f64::from(x.scale[2]), 0.0]]).collect();
        let matrix: Vec<Option<M4>> = g.nodes.iter().map(|x| x.matrix.map(f64s)).collect();
        let mut rig = Rig { names: g.nodes.iter().map(|x| x.name.clone()).collect(), parent, order, rest, matrix, shell_w: vec![MIN_SHELL_M; n], shell_l: vec![MIN_SHELL_M; n], parent_scale: vec![1.0; n] };
        let world = rig.world(&|_, _| None);
        for i in 0..n {
            let ws = max_scale(&world[i]).max(1e-9);
            rig.parent_scale[i] = rig.parent[i].map_or(1.0, |p| max_scale(&world[p]));
            let me = point(&world[i], [0.0; 3]);
            let mut d = MIN_SHELL_M;
            for &c in &g.nodes[i].children {
                d = d.max(dist(me, point(&world[c], [0.0; 3])));
            }
            if g.nodes[i].skin.is_none()
                && let Some(m) = g.nodes[i].mesh.and_then(|m| g.meshes.get(m))
            {
                for p in &m.primitives {
                    let Some(a) = p.attribute("POSITION").and_then(|a| g.accessors.get(a)) else { continue };
                    if a.min.len() == 3 && a.max.len() == 3 {
                        let r = (0..3).map(|k| f64::from(a.min[k].abs().max(a.max[k].abs())).powi(2)).sum::<f64>().sqrt();
                        d = d.max(r * ws);
                    }
                }
            }
            rig.shell_w[i] = d;
            rig.shell_l[i] = d / ws;
        }
        rig
    }

    fn rest_value(&self, n: usize, ch: u8) -> V4 {
        self.rest[n][usize::from(ch)]
    }

    /// World matrices with `over(node, channel)` replacing rest values.
    fn world(&self, over: &dyn Fn(usize, u8) -> Option<V4>) -> Vec<M4> {
        let mut w = vec![ident(); self.names.len()];
        for &i in &self.order {
            let local = match &self.matrix[i] {
                Some(m) => *m,
                None => {
                    let [t, r, s] = [TRANSLATION, ROTATION, SCALE].map(|c| over(i, c).unwrap_or_else(|| self.rest_value(i, c)));
                    compose(&t, &r, &s)
                }
            };
            w[i] = match self.parent[i] {
                Some(p) => mul(&w[p], &local),
                None => local,
            };
        }
        w
    }

    /// One track's own error between two values, in metres at the shell.
    fn err(&self, n: usize, ch: u8, a: &V4, b: &V4) -> f64 {
        match ch {
            TRANSLATION => dist([a[0], a[1], a[2]], [b[0], b[1], b[2]]) * self.parent_scale[n],
            ROTATION => {
                let la = (a.iter().map(|x| x * x).sum::<f64>()).sqrt().max(1e-12);
                let lb = (b.iter().map(|x| x * x).sum::<f64>()).sqrt().max(1e-12);
                let dot = ((0..4).map(|k| a[k] * b[k]).sum::<f64>() / (la * lb)).abs().min(1.0);
                2.0 * (1.0 - dot * dot).max(0.0).sqrt() * self.shell_w[n]
            }
            _ => {
                let r = self.rest_value(n, SCALE);
                (0..3).map(|k| (a[k] - b[k]).abs() / r[k].abs().max(1e-6)).fold(0.0, f64::max) * self.shell_w[n]
            }
        }
    }
}

/// One animated channel, resampled.
struct Chan {
    node: usize,
    tier: u16,
    channel: u8,
    v: Vec<V4>,
}

/// One clip in the report.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipReport {
    pub name: String,
    pub frames: u16,
    /// Stored tracks (keyed and constant).
    pub tracks: usize,
    /// Tracks stored once (one value).
    pub constant: usize,
    /// Animated channels dropped because they stay at the rest pose.
    pub rest: usize,
    /// Keys over all stored tracks.
    pub keys: usize,
    /// 32 (10-10-10-2) or 48 (smallest-three).
    pub rot_bits: u32,
    pub bytes: usize,
    pub worst_shell_error_m: f64,
    pub worst_node: String,
    pub within: bool,
}

/// A baked block and its report.
#[derive(Clone, Debug, PartialEq)]
pub struct Baked {
    pub block: Vec<u8>,
    pub bound_m: f64,
    pub clips: Vec<ClipReport>,
    pub notes: Vec<String>,
}

/// Index of the last key at or before `t` (0 when `t` is before the first).
fn key_at(times: &[f32], t: f64) -> usize {
    times.partition_point(|&x| f64::from(x) <= t).saturating_sub(1)
}

fn slerp(a: V4, b: V4, u: f64) -> V4 {
    let mut dot: f64 = (0..4).map(|k| a[k] * b[k]).sum();
    let b = if dot < 0.0 {
        dot = -dot;
        b.map(|x| -x)
    } else {
        b
    };
    let (wa, wb) = if dot > 0.9995 {
        (1.0 - u, u)
    } else {
        let th = dot.min(1.0).acos();
        let s = th.sin();
        (((1.0 - u) * th).sin() / s, (u * th).sin() / s)
    };
    norm([0, 1, 2, 3].map(|k| a[k] * wa + b[k] * wb))
}

fn norm(q: V4) -> V4 {
    let l = (q.iter().map(|x| x * x).sum::<f64>()).sqrt();
    if l > 0.0 && l.is_finite() { q.map(|x| x / l) } else { [0.0, 0.0, 0.0, 1.0] }
}

/// A sampler at `frames` times f / 30 (clamped to its keys).
fn resample(g: &Gltf, s: &AnimSampler, comps: usize, frames: u16, what: &str) -> Result<Vec<V4>, String> {
    let times = g.read_f32(s.input).map_err(|e| format!("{what}: key times: {}", e.0))?;
    let vals = g.read_f32(s.output).map_err(|e| format!("{what}: key values: {}", e.0))?;
    let cubic = s.interpolation == "CUBICSPLINE";
    if !matches!(s.interpolation.as_str(), "LINEAR" | "STEP" | "CUBICSPLINE") {
        return Err(format!("{what}: unknown interpolation {}", s.interpolation));
    }
    let per = if cubic { 3 * comps } else { comps };
    if times.is_empty() || vals.len() != times.len() * per {
        return Err(format!("{what}: {} key times but {} values ({} per key expected)", times.len(), vals.len(), per));
    }
    if times.windows(2).any(|w| w[1] < w[0]) || times.iter().any(|t| !t.is_finite()) {
        return Err(format!("{what}: key times are not increasing"));
    }
    // Element e of key k: for CUBICSPLINE 0 is the in-tangent, 1 the value, 2 the out-tangent.
    let get = |k: usize, e: usize| -> V4 {
        let base = k * per + if cubic { e * comps } else { 0 };
        let mut v = [0.0; 4];
        for (c, x) in v.iter_mut().enumerate().take(comps) {
            *x = f64::from(vals[base + c]);
        }
        v
    };
    let rot = comps == 4;
    let last = times.len() - 1;
    let mut out = Vec::with_capacity(usize::from(frames));
    for f in 0..frames {
        let t = f64::from(f) / f64::from(clipblock::RATE);
        let k = key_at(&times, t);
        let v = if k >= last || t <= f64::from(times[0]) {
            get(if t <= f64::from(times[0]) { 0 } else { last }, 1)
        } else {
            let (t0, t1) = (f64::from(times[k]), f64::from(times[k + 1]));
            let u = if t1 > t0 { ((t - t0) / (t1 - t0)).clamp(0.0, 1.0) } else { 0.0 };
            match s.interpolation.as_str() {
                "STEP" => get(k, 1),
                "LINEAR" if rot => slerp(norm(get(k, 1)), norm(get(k + 1, 1)), u),
                "LINEAR" => {
                    let (a, b) = (get(k, 1), get(k + 1, 1));
                    [0, 1, 2, 3].map(|c| a[c] + (b[c] - a[c]) * u)
                }
                _ => {
                    let dt = t1 - t0;
                    let (p0, m0, p1, m1) = (get(k, 1), get(k, 2), get(k + 1, 1), get(k + 1, 0));
                    let (u2, u3) = (u * u, u * u * u);
                    let (h00, h10, h01, h11) = (2.0 * u3 - 3.0 * u2 + 1.0, u3 - 2.0 * u2 + u, -2.0 * u3 + 3.0 * u2, u3 - u2);
                    let v = [0, 1, 2, 3].map(|c| h00 * p0[c] + h10 * dt * m0[c] + h01 * p1[c] + h11 * dt * m1[c]);
                    if rot { norm(v) } else { v }
                }
            }
        };
        out.push(if rot { norm(v) } else { v });
    }
    if rot {
        // One hemisphere along the clip (same rotations, steadier keys).
        for f in 1..out.len() {
            if (0..4).map(|c| out[f][c] * out[f - 1][c]).sum::<f64>() < 0.0 {
                out[f] = out[f].map(|x| -x);
            }
        }
    }
    Ok(out)
}

/// What one track became at one tolerance.
enum Reduced {
    Rest,
    Track(TrackIn),
}

/// The decoder's view of a value after quantisation.
fn quantised(ch: u8, v: &V4, rot_bytes: u8, range: Option<([f32; 3], [f32; 3])>) -> Result<V4, String> {
    let f = v.map(|x| x as f32);
    Ok(match (ch, range) {
        (ROTATION, _) => v4(clipblock::decode_rot(clipblock::encode_rot(f, rot_bytes)?, rot_bytes)?),
        (_, Some((min, ext))) => {
            let mut o = [0.0; 4];
            for k in 0..3 {
                o[k] = f64::from(clipblock::dequant_u16(clipblock::quant_u16(f[k], min[k], ext[k]), min[k], ext[k]));
            }
            o
        }
        _ => v4(f),
    })
}

fn blend(ch: u8, a: &V4, b: &V4, u: f64) -> V4 {
    v4(clipblock::blend(ch, a.map(|x| x as f32), b.map(|x| x as f32), u as f32))
}

/// One channel at tolerance `tau` (metres at the shell).
fn reduce(rig: &Rig, c: &Chan, tau: f64, rot_bytes: u8) -> Result<Reduced, String> {
    let tau = tau.max(EPS);
    let rest = rig.rest_value(c.node, c.channel);
    if c.v.iter().all(|v| rig.err(c.node, c.channel, v, &rest) <= tau) {
        return Ok(Reduced::Rest);
    }
    let first = c.v[0];
    let f32v = |v: &V4| [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32];
    if c.v.iter().all(|v| rig.err(c.node, c.channel, v, &first) <= tau) {
        return Ok(Reduced::Track(TrackIn { node: c.tier, channel: c.channel, keys: vec![0], values: vec![f32v(&first)], range: None }));
    }
    let range = if c.channel == ROTATION {
        None
    } else {
        let vals: Vec<[f32; 4]> = c.v.iter().map(f32v).collect();
        Some(clipblock::range_of(&vals))
    };
    let q: Vec<V4> = c.v.iter().map(|v| quantised(c.channel, v, rot_bytes, range)).collect::<Result<_, _>>()?;
    let last = c.v.len() - 1;
    let fits = |a: usize, b: usize| (a + 1..b).all(|f| rig.err(c.node, c.channel, &blend(c.channel, &q[a], &q[b], (f - a) as f64 / (b - a) as f64), &c.v[f]) <= tau);
    // Greedy forward pass: from each key, the farthest next key that still
    // fits, found by doubling the reach and then bisecting, so a long smooth
    // segment costs about L log L frame checks rather than L squared.
    let mut keys = vec![0usize];
    let mut a = 0;
    while a < last {
        let mut good = a + 1;
        let mut bad = None;
        while good < last {
            let next = (a + 2 * (good - a)).min(last);
            if fits(a, next) {
                good = next;
            } else {
                bad = Some(next);
                break;
            }
        }
        if let Some(mut bad) = bad {
            while bad - good > 1 {
                let mid = good + (bad - good) / 2;
                if fits(a, mid) {
                    good = mid;
                } else {
                    bad = mid;
                }
            }
        }
        keys.push(good);
        a = good;
    }
    Ok(Reduced::Track(TrackIn {
        node: c.tier,
        channel: c.channel,
        keys: keys.iter().map(|&k| k as u16).collect(),
        values: keys.iter().map(|&k| f32v(&c.v[k])).collect(),
        range,
    }))
}

/// The worst shell-point distance (metres) between the decoded clip and the
/// resampled source, over every frame and every node at or below an
/// animated one: (error, master node).
fn measure(rig: &Rig, chans: &[Chan], affected: &[usize], reference: &[Vec<[f64; 3]>], bytes: &[u8]) -> Result<(f64, usize), String> {
    let clip = clipblock::parse_clip(bytes).map_err(|e| format!("the baked clip does not read back: {e}"))?;
    let back: BTreeMap<u16, usize> = chans.iter().map(|c| (c.tier, c.node)).collect();
    let mut tracks: BTreeMap<(usize, u8), &clipblock::Track> = BTreeMap::new();
    for t in &clip.tracks {
        let n = back.get(&t.node).copied().ok_or_else(|| format!("the baked clip names tier node {} that no channel targets", t.node))?;
        tracks.insert((n, t.channel), t);
    }
    let (mut worst, mut at) = (0.0f64, affected.first().copied().unwrap_or(0));
    for (f, refs) in reference.iter().enumerate() {
        let world = rig.world(&|n, ch| tracks.get(&(n, ch)).map(|t| v4(t.sample_frame(f as f32))));
        for (k, &n) in affected.iter().enumerate() {
            for (j, p) in shell_points(rig, n).into_iter().enumerate() {
                let e = dist(point(&world[n], p), refs[k * 6 + j]);
                if e > worst {
                    worst = e;
                    at = n;
                }
            }
        }
    }
    Ok((worst, at))
}

fn shell_points(rig: &Rig, n: usize) -> [[f64; 3]; 6] {
    let d = rig.shell_l[n];
    [[d, 0.0, 0.0], [-d, 0.0, 0.0], [0.0, d, 0.0], [0.0, -d, 0.0], [0.0, 0.0, d], [0.0, 0.0, -d]]
}

/// Bake every animation of `g` (the master, with node names that `map` can
/// find in the tier file) into one clip block. `map` turns a master node
/// into the tier file's node index; `bound_m` is the worst shell error
/// allowed (metres).
pub fn bake(g: &Gltf, bound_m: f64, map: &dyn Fn(usize) -> Option<usize>) -> Result<Baked, String> {
    if !(bound_m.is_finite() && bound_m > 0.0) {
        return Err(format!("clip error bound {bound_m} m: a distance above 0"));
    }
    let rig = Rig::new(g);
    let mut notes = Vec::new();
    let mut reports = Vec::new();
    let mut blobs = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for (ai, a) in g.animations.iter().enumerate() {
        let mut name = if a.name.is_empty() { format!("clip_{ai}") } else { a.name.clone() };
        if names.contains(&name) {
            let mut k = ai;
            let mut new = format!("{name}_{k}");
            // Never a name another animation already has (or will have).
            while names.contains(&new) || g.animations.iter().any(|b| b.name == new) {
                k += 1;
                new = format!("{name}_{k}");
            }
            notes.push(format!("animation {ai}: the name `{name}` is taken, stored as `{new}`"));
            name = new;
        }
        names.push(name.clone());
        // Channels by (node, channel): the first wins.
        let mut picked: BTreeMap<(usize, u8), usize> = BTreeMap::new();
        for (ci, c) in a.channels.iter().enumerate() {
            let ch = match c.path.as_str() {
                "translation" => TRANSLATION,
                "rotation" => ROTATION,
                "scale" => SCALE,
                "weights" => {
                    notes.push(format!("clip `{name}`: morph target weights are not baked (no morph targets in the engine yet); channel {ci} skipped"));
                    continue;
                }
                p => {
                    notes.push(format!("clip `{name}`: channel {ci} animates `{p}`, which is not baked; skipped"));
                    continue;
                }
            };
            let Some(n) = c.node else {
                notes.push(format!("clip `{name}`: channel {ci} targets no node; skipped"));
                continue;
            };
            if g.nodes[n].matrix.is_some() {
                return Err(format!("clip `{name}`: node `{}` has a matrix and is animated (glTF forbids it; export it with translation, rotation and scale)", g.nodes[n].name));
            }
            if picked.contains_key(&(n, ch)) {
                notes.push(format!("clip `{name}`: node `{}` {} is animated twice; the first channel is kept", g.nodes[n].name, channel_name(ch)));
                continue;
            }
            picked.insert((n, ch), c.sampler);
        }
        // Length: the last key time of any channel.
        let mut end = 0.0f64;
        for &s in picked.values() {
            let t = g.read_f32(a.samplers[s].input).map_err(|e| format!("clip `{name}`: key times: {}", e.0))?;
            end = t.iter().fold(end, |m, &x| m.max(f64::from(x)));
        }
        let frames = (end * f64::from(clipblock::RATE)).round() + 1.0;
        if frames > f64::from(u16::MAX) {
            return Err(format!("clip `{name}`: {end:.1} s is over the {} frames a clip can hold", u16::MAX));
        }
        let frames = frames as u16;
        let mut chans = Vec::new();
        for (&(n, ch), &s) in &picked {
            let tier = map(n).ok_or_else(|| format!("clip `{name}`: the animated node `{}` is not in the tier file", rig.names[n]))?;
            let tier = u16::try_from(tier).map_err(|_| format!("clip `{name}`: tier node {tier} is past 65535"))?;
            let what = format!("clip `{name}`: node `{}` {}", rig.names[n], channel_name(ch));
            let v = resample(g, &a.samplers[s], if ch == ROTATION { 4 } else { 3 }, frames, &what)?;
            chans.push(Chan { node: n, tier, channel: ch, v });
        }
        chans.sort_by_key(|c| (c.tier, c.channel));
        if let Some(w) = chans.windows(2).find(|w| (w[0].tier, w[0].channel) == (w[1].tier, w[1].channel)) {
            return Err(format!("clip `{name}`: nodes `{}` and `{}` are the same node in the tier file", rig.names[w[0].node], rig.names[w[1].node]));
        }
        let (blob, rep, why) = bake_clip(&rig, &name, frames, &chans, bound_m)?;
        notes.extend(why);
        blobs.push(blob);
        reports.push(rep);
    }
    Ok(Baked { block: clipblock::write_block(&blobs)?, bound_m, clips: reports, notes })
}

/// The smallest clip within the bound (or, when none is, the most accurate).
fn bake_clip(rig: &Rig, name: &str, frames: u16, chans: &[Chan], bound: f64) -> Result<(Vec<u8>, ClipReport, Vec<String>), String> {
    // Every node at or below an animated one, and its reference shell points.
    let animated: Vec<usize> = chans.iter().map(|c| c.node).collect();
    let affected: Vec<usize> = rig.order.iter().copied().filter(|&n| {
        let mut x = Some(n);
        while let Some(i) = x {
            if animated.contains(&i) {
                return true;
            }
            x = rig.parent[i];
        }
        false
    }).collect();
    let by: BTreeMap<(usize, u8), &Chan> = chans.iter().map(|c| ((c.node, c.channel), c)).collect();
    let mut reference = Vec::with_capacity(usize::from(frames));
    for f in 0..usize::from(frames) {
        let world = rig.world(&|n, ch| by.get(&(n, ch)).map(|c| c.v[f]));
        reference.push(affected.iter().flat_map(|&n| shell_points(rig, n).map(|p| point(&world[n], p))).collect::<Vec<_>>());
    }
    let mut best: Option<(Vec<u8>, ClipReport)> = None;
    for rot_bytes in [4u8, 6] {
        for k in 0..=STEPS {
            let tau = if k == STEPS { 0.0 } else { bound / f64::from(1u32 << k) };
            let mut tracks = Vec::new();
            let mut rest = 0;
            for c in chans {
                match reduce(rig, c, tau, rot_bytes)? {
                    Reduced::Rest => rest += 1,
                    Reduced::Track(t) => tracks.push(t),
                }
            }
            let constant = tracks.iter().filter(|t| t.keys.len() == 1).count();
            let keys = tracks.iter().map(|t| t.keys.len()).sum();
            let n_tracks = tracks.len();
            let bytes = clipblock::write_clip(&ClipIn { frames, rot_bytes, tracks })?;
            let (err, at) = measure(rig, chans, &affected, &reference, &bytes)?;
            let within = err <= bound;
            let rep = ClipReport {
                name: name.to_owned(),
                frames,
                tracks: n_tracks,
                constant,
                rest,
                keys,
                rot_bits: u32::from(rot_bytes) * 8,
                bytes: bytes.len(),
                worst_shell_error_m: err,
                worst_node: rig.names[at].clone(),
                within,
            };
            let better = match &best {
                None => true,
                Some((b, r)) => match (within, r.within) {
                    (true, false) => true,
                    (true, true) => bytes.len() < b.len(),
                    (false, false) => err < r.worst_shell_error_m,
                    (false, true) => false,
                },
            };
            if better {
                best = Some((bytes, rep));
            }
            if within {
                break;
            }
        }
    }
    let (bytes, rep) = best.ok_or_else(|| format!("clip `{name}`: nothing baked"))?;
    let notes = if rep.within { Vec::new() } else { over_causes(rig, name, chans, bound, &rep) };
    Ok((bytes, rep, notes))
}

/// Why a clip stays over its bound even with every key kept: the 16-bit
/// steps of a long translation or scale track, or 48-bit rotations far from
/// their joint.
fn over_causes(rig: &Rig, name: &str, chans: &[Chan], bound: f64, rep: &ClipReport) -> Vec<String> {
    let mut out = Vec::new();
    for c in chans.iter().filter(|c| c.channel != ROTATION) {
        let vals: Vec<[f32; 4]> = c.v.iter().map(|v| v.map(|x| x as f32)).collect();
        let (_, ext) = clipblock::range_of(&vals);
        // Half a 16-bit step, in metres at the shell.
        let per = if c.channel == TRANSLATION { rig.parent_scale[c.node] } else { rig.shell_w[c.node] };
        let floor = (0..3)
            .map(|k| {
                let unit = if c.channel == SCALE { rig.rest_value(c.node, SCALE)[k].abs().max(1e-6) } else { 1.0 };
                f64::from(ext[k]) / 131_070.0 / unit * per
            })
            .fold(0.0, f64::max);
        if floor > bound {
            let span = ext.iter().fold(0.0f32, |m, &x| m.max(x));
            out.push(format!(
                "clip `{name}`: node `{}` {} spans {span:.3} over the clip, so its 16-bit steps alone are off by up to {:.4} mm, over the {:.4} mm bound; the track's range is the limit, not the rotation width (loosen the bound with --clip-error, or split the clip)",
                rig.names[c.node],
                channel_name(c.channel),
                floor * 1000.0,
                bound * 1000.0
            ));
        }
    }
    if out.is_empty() {
        out.push(format!(
            "clip `{name}`: over the {:.4} mm bound with every key kept ({:.4} mm at node `{}`, {}-bit rotations): the bound is finer than the stored rotations can hold at that distance from the joint; loosen it with --clip-error",
            bound * 1000.0,
            rep.worst_shell_error_m * 1000.0,
            rep.worst_node,
            rep.rot_bits
        ));
    }
    out
}
