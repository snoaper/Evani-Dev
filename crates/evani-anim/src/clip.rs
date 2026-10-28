//! The clip decoder and sampler: the `EVC1` block a tier file carries
//! (docs/MODEL_FORMAT.md "Clips"; the reference reader is
//! `tools/evani-guide/src/clipblock.rs`, which this follows step for step).
//!
//! The whole block is checked and decoded once, at load, into flat key and
//! value tables shared by every character that uses the model. Sampling then
//! cannot fail and never allocates: a [`Cursor`] remembers each track's last
//! key, so playing forward costs one or two comparisons per track.

use crate::math::{Quat, QID, Xform, mix4, qnorm};
use crate::Error;

/// Clip samples per second (the block's `rate`).
pub const RATE: u32 = 30;
/// Simulation ticks per clip frame (60 Hz step, 30 Hz clips).
pub const TICKS_PER_FRAME: u32 = 2;

pub const TRANSLATION: u8 = 0;
pub const ROTATION: u8 = 1;
pub const SCALE: u8 = 2;

const HALF_SQRT2: f32 = core::f32::consts::FRAC_1_SQRT_2;

/// One track: the node and channel it drives and its run in the key tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Track {
    pub node: u16,
    pub channel: u8,
    first: u32,
    count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClipInfo {
    frames: u16,
    first_track: u32,
    tracks: u16,
}

/// Every clip of one block, decoded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clips {
    clips: Vec<ClipInfo>,
    tracks: Vec<Track>,
    keys: Vec<u16>,
    values: Vec<[f32; 4]>,
    max_tracks: usize,
}

/// Per-track key positions for sampling one clip after another without
/// searching. Made once per player with [`Clips::cursor`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    clip: usize,
    keys: Vec<u16>,
}

struct Rd<'a> {
    b: &'a [u8],
    at: usize,
}

const SHORT: Error = err!("evc1 short", "clip block: data runs past the end");

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Error> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len()).ok_or(SHORT)?;
        let s = self.b.get(self.at..end).ok_or(SHORT)?;
        self.at = end;
        Ok(s)
    }
    /// A little-endian unsigned integer of `n` bytes (1 to 8).
    fn uint(&mut self, n: usize) -> Result<u64, Error> {
        Ok(self.take(n)?.iter().rev().fold(0u64, |a, &x| (a << 8) | u64::from(x)))
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.uint(1)? as u8)
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(self.uint(2)? as u16)
    }
    fn f32(&mut self) -> Result<f32, Error> {
        let v = f32::from_bits(self.uint(4)? as u32);
        if v.is_finite() { Ok(v) } else { Err(err!("evc1 number", "clip block: a number that is not finite")) }
    }
    fn f3(&mut self) -> Result<[f32; 3], Error> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
}

/// A smallest-three rotation of `rot_bytes` bytes (4: 10-10-10-2, 6: 3 x 15).
fn decode_rot(bits_in: u64, rot_bytes: u8) -> Quat {
    let bits = if rot_bytes == 4 { 10 } else { 15 };
    let max = (1u64 << bits) - 1;
    let big = (bits_in & 3) as usize;
    let mut q = [0.0f32; 4];
    let mut shift = 2;
    let mut sum = 0.0f32;
    for (i, c) in q.iter_mut().enumerate() {
        if i == big {
            continue;
        }
        let k = (bits_in >> shift) & max;
        *c = (k as f32 / max as f32 * 2.0 - 1.0) * HALF_SQRT2;
        sum += *c * *c;
        shift += bits;
    }
    q[big] = (1.0 - sum).max(0.0).sqrt();
    qnorm(q)
}

impl Clips {
    /// Check and decode a whole `EVC1` block. Bad bytes are an error, never
    /// a panic. Keeps the `evani.module.anim` marker in the download.
    pub fn decode(block: &[u8]) -> Result<Self, Error> {
        core::hint::black_box(crate::MODULE_ID);
        let mut r = Rd { b: block, at: 0 };
        let magic = r.take(4)? != b"EVC1";
        let count = r.u16()?;
        if magic || r.u16()? != 0 {
            return Err(err!("evc1 header", "clip block: no EVC1 magic, or its reserved bytes are not 0"));
        }
        let mut out = Clips::default();
        let head = 8 + 4 * usize::from(count);
        let mut offsets = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let o = r.uint(4)? as usize;
            if !o.is_multiple_of(4) || o >= block.len() || o < head {
                return Err(err!("evc1 offset", "clip block: a clip offset is not a 4-byte aligned place after the header"));
            }
            offsets.push(o);
        }
        for o in offsets {
            out.clip(&mut Rd { b: block, at: o })?;
        }
        Ok(out)
    }

    fn clip(&mut self, r: &mut Rd) -> Result<(), Error> {
        let frames = r.u16()?;
        let n = r.u16()?;
        let rot_bytes = r.u8()?;
        let ib = r.u8()?;
        // One error per header kind: fewer branches in the wasm.
        if r.u16()? != 0 || frames == 0 || (rot_bytes != 4 && rot_bytes != 6) || ib != if frames <= 256 { 1 } else { 2 } {
            return Err(err!("evc1 clip header", "clip block: a clip header with reserved bytes not 0, 0 frames, rot_bytes not 4 or 6, or index_bytes not matching the frame count"));
        }
        let first_track = self.tracks.len();
        let mut prev: Option<(u16, u8)> = None;
        for _ in 0..n {
            let node = r.u16()?;
            let count = r.u16()?;
            let channel = r.u8()?;
            if r.u8()? != 0 || channel > SCALE || count == 0 || count > frames || prev.is_some_and(|p| p >= (node, channel)) {
                return Err(err!("evc1 track header", "clip block: a track header with its reserved byte not 0, a channel not 0, 1 or 2, 0 keys or more keys than frames, or out of (node, channel) order or repeated"));
            }
            prev = Some((node, channel));
            self.tracks.push(Track { node, channel, first: 0, count });
        }
        for t in &mut self.tracks[first_track..] {
            t.first = u32::try_from(self.keys.len()).map_err(|_| err!("evc1 size", "clip block: too many keys"))?;
            if t.count == 1 {
                self.keys.push(0);
                let v = if t.channel == ROTATION { qnorm([r.f32()?, r.f32()?, r.f32()?, r.f32()?]) } else { let [x, y, z] = r.f3()?; [x, y, z, 0.0] };
                self.values.push(v);
                continue;
            }
            let range = if t.channel == ROTATION { None } else { Some((r.f3()?, r.f3()?)) };
            let mut last: Option<u16> = None;
            for k in 0..t.count {
                let f = r.uint(usize::from(ib))? as u16;
                let ok = match last {
                    None => f == 0,
                    Some(p) => f > p,
                };
                if !ok || (k + 1 == t.count && f != frames - 1) {
                    return Err(err!("evc1 key frames", "clip block: key frames must run 0 .. frames - 1, strictly increasing"));
                }
                last = Some(f);
                self.keys.push(f);
            }
            for _ in 0..t.count {
                let v = match range {
                    None => decode_rot(r.uint(usize::from(rot_bytes))?, rot_bytes),
                    Some((min, ext)) => {
                        let (a, b, c) = (r.u16()?, r.u16()?, r.u16()?);
                        [min[0] + ext[0] * f32::from(a) / 65535.0, min[1] + ext[1] * f32::from(b) / 65535.0, min[2] + ext[2] * f32::from(c) / 65535.0, 0.0]
                    }
                };
                self.values.push(v);
            }
        }
        self.max_tracks = self.max_tracks.max(usize::from(n));
        self.clips.push(ClipInfo { frames, first_track: first_track as u32, tracks: n });
        Ok(())
    }

    /// How many clips.
    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// A clip's frame count (30 Hz samples), or 0 for a clip that isn't there.
    pub fn frames(&self, clip: usize) -> u16 {
        self.clips.get(clip).map_or(0, |c| c.frames)
    }

    /// A clip's length in simulation ticks ((frames - 1) x 2).
    pub fn ticks(&self, clip: usize) -> u32 {
        u32::from(self.frames(clip).saturating_sub(1)) * TICKS_PER_FRAME
    }

    /// A clip's tracks, sorted by node and channel.
    pub fn tracks(&self, clip: usize) -> &[Track] {
        let Some(c) = self.clips.get(clip) else { return &[] };
        let a = c.first_track as usize;
        self.tracks.get(a..a + usize::from(c.tracks)).unwrap_or(&[])
    }

    /// The highest node index any track drives, plus one (0 with no tracks):
    /// the smallest skeleton these clips fit.
    pub fn nodes_needed(&self) -> usize {
        self.tracks.iter().map(|t| usize::from(t.node) + 1).max().unwrap_or(0)
    }

    /// A cursor for sampling these clips.
    pub fn cursor(&self) -> Cursor {
        Cursor { clip: usize::MAX, keys: vec![0; self.max_tracks] }
    }

    /// A track's key frame numbers and values (rotations x y z w; the first
    /// three for translations and scales).
    pub fn keys(&self, t: &Track) -> (&[u16], &[[f32; 4]]) {
        let a = t.first as usize;
        let b = a + usize::from(t.count);
        (self.keys.get(a..b).unwrap_or(&[]), self.values.get(a..b).unwrap_or(&[]))
    }

    /// A track's value at frame position `x` (fractional 30 Hz frames,
    /// clamped to the clip), starting the key search at `*key` and leaving
    /// the key found there. Identical to the reference reader's sample.
    pub fn sample_track(&self, t: &Track, x: f32, key: &mut u16) -> [f32; 4] {
        let (keys, values) = self.keys(t);
        let n = keys.len().min(values.len());
        if n == 0 {
            return QID;
        }
        if n == 1 || x <= 0.0 {
            *key = 0;
            return values[0];
        }
        let x = x.min(f32::from(keys[n - 1]));
        let mut i = usize::from(*key).min(n - 1);
        while i > 0 && f32::from(keys[i]) > x {
            i -= 1;
        }
        while i + 1 < n && f32::from(keys[i + 1]) <= x {
            i += 1;
        }
        *key = i as u16;
        if i + 1 >= n {
            return values[n - 1];
        }
        let (f0, f1) = (f32::from(keys[i]), f32::from(keys[i + 1]));
        let u = (x - f0) / (f1 - f0);
        let rot = t.channel == ROTATION;
        let v = mix4(values[i], values[i + 1], u, rot);
        if rot { qnorm(v) } else { v }
    }

    /// Write `clip` at frame position `x` into `pose` (one transform per
    /// node; start it from the rest pose, since nodes without a track keep
    /// what they hold). Tracks for nodes past the end of `pose` are skipped;
    /// [`Clips::nodes_needed`] says how large a pose they need.
    pub fn sample(&self, clip: usize, x: f32, cursor: &mut Cursor, pose: &mut [Xform]) {
        if cursor.clip != clip {
            cursor.clip = clip;
            cursor.keys.iter_mut().for_each(|k| *k = 0);
        }
        for (i, t) in self.tracks(clip).iter().enumerate() {
            let mut spare = 0;
            let key = cursor.keys.get_mut(i).unwrap_or(&mut spare);
            let v = self.sample_track(t, x, key);
            let Some(p) = pose.get_mut(usize::from(t.node)) else { continue };
            match t.channel {
                TRANSLATION => p.t = [v[0], v[1], v[2]],
                ROTATION => p.r = v,
                _ => p.s = [v[0], v[1], v[2]],
            }
        }
    }
}

/// The frame position at `ticks` simulation ticks into a clip of `frames`
/// frames, plus `sub` of the next tick (render interpolation, 0 to 1; the
/// simulation itself passes 0). A looped clip wraps at its last frame.
pub fn frame_at(ticks: u32, sub: f32, frames: u16, looped: bool) -> f32 {
    let len = u32::from(frames.saturating_sub(1)) * TICKS_PER_FRAME;
    if len == 0 {
        return 0.0;
    }
    let last = f32::from(frames - 1);
    if looped {
        ((ticks % len) as f32 + sub) * 0.5
    } else if ticks >= len {
        last
    } else {
        ((ticks as f32 + sub) * 0.5).min(last)
    }
}

/// The frame position at `phase` (a whole cycle is 2^32) of a looped clip.
pub fn frame_at_phase(phase: u32, frames: u16) -> f32 {
    (phase as f32 / 4_294_967_296.0) * f32::from(frames.saturating_sub(1))
}
