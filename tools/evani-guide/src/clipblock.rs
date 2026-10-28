//! The compact animation clip block (STAGE3_PLAN S3-7; docs/MODEL_FORMAT.md
//! "Clips"): the reference reader and writer of the bytes evani-convert bakes
//! into a tier file's binary chunk and S3-8's in-game decoder reads.
//!
//! One source for three callers: the converter's encoder writes with
//! [`write`] and measures its error by reading its own bytes back with
//! [`parse`], the shared model check ([`crate::modelcheck`]) refuses a tier
//! file whose block does not parse, and S3-8's decoder follows the same
//! layout. Fixed little-endian layouts, no general-purpose compression, so
//! the game-side decoder stays small (about 1 to 2 KB of wasm).
//!
//! Layout (all little-endian):
//! - Block: magic `EVC1`, u16 clip count, u16 reserved (0), then one u32
//!   offset per clip, from the block's start, each a multiple of 4.
//! - Clip: u16 frames (samples at 30 Hz; duration (frames - 1) / 30 s), u16
//!   tracks, u8 rot_bytes (4: 10-10-10-2, or 6: smallest-three 3 x 15 bits),
//!   u8 index_bytes (1 when frames <= 256, else 2), u16 reserved (0).
//! - Track headers, 6 bytes each, sorted by (node, channel), no repeats:
//!   u16 node (index in the tier file), u16 keys (1..=frames), u8 channel
//!   (0 translation, 1 rotation, 2 scale), u8 reserved (0).
//! - Track data, in header order:
//!   - keys = 1: a constant, f32 x 3 (f32 x 4, x y z w, for a rotation).
//!   - keys >= 2: translation and scale first carry f32 x 3 min and f32 x 3
//!     extent; then the key frame numbers (index_bytes each, the first 0,
//!     the last frames - 1, strictly increasing); then the values: a
//!     rotation in rot_bytes, a translation or scale as u16 x 3 with
//!     v = min + extent * q / 65535.
//! - Rotation bits (an unsigned integer of rot_bytes): bits 0-1 name the
//!   dropped (largest) component; then the other three in x y z w order,
//!   10 or 15 bits each, each `q / max * 2 - 1` times 1/sqrt(2). The dropped
//!   component is sqrt(1 - sum of the squares of the other three).
//! - Sampling at time t: x = t * 30 clamped to [0, frames - 1]; between the
//!   two keys around x, translation and scale lerp, rotation nlerps (the
//!   second key negated when the dot product is below 0, then normalised).
//!   A node with no track keeps its rest transform from the file.
//!
//! A native dev tool: nothing here reaches a game's download.

/// The block's first four bytes.
pub const MAGIC: [u8; 4] = *b"EVC1";
/// Samples per second.
pub const RATE: u32 = 30;
/// `extras.evani.clips.format`.
pub const FORMAT: u64 = 1;

pub const TRANSLATION: u8 = 0;
pub const ROTATION: u8 = 1;
pub const SCALE: u8 = 2;

const HALF_SQRT2: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// One decoded track: key frames and their decoded values (rotations x y z
/// w; translations and scales in the first three).
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub node: u16,
    pub channel: u8,
    pub keys: Vec<u16>,
    pub values: Vec<[f32; 4]>,
    /// Bytes of this track's data (not its header).
    pub bytes: usize,
}

/// One decoded clip.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub frames: u16,
    pub rot_bytes: u8,
    pub index_bytes: u8,
    pub tracks: Vec<Track>,
    /// Bytes of the clip: header, track headers and data.
    pub bytes: usize,
}

impl Clip {
    pub fn duration(&self) -> f32 {
        f32::from(self.frames.saturating_sub(1)) / RATE as f32
    }
}

/// A track to write: `keys` and `values` as [`Track`]; translation and scale
/// tracks with two or more keys also carry their quantisation range.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackIn {
    pub node: u16,
    pub channel: u8,
    pub keys: Vec<u16>,
    pub values: Vec<[f32; 4]>,
    /// (min, extent) for translation and scale with keys >= 2.
    pub range: Option<([f32; 3], [f32; 3])>,
}

/// A clip to write.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipIn {
    pub frames: u16,
    pub rot_bytes: u8,
    pub tracks: Vec<TrackIn>,
}

pub fn index_bytes(frames: u16) -> u8 {
    if frames <= 256 { 1 } else { 2 }
}

/// Bits per stored rotation component.
fn rot_bits(rot_bytes: u8) -> Option<u32> {
    match rot_bytes {
        4 => Some(10),
        6 => Some(15),
        _ => None,
    }
}

/// A unit quaternion as `rot_bytes` bytes (smallest-three).
pub fn encode_rot(q: [f32; 4], rot_bytes: u8) -> Result<u64, String> {
    let bits = rot_bits(rot_bytes).ok_or_else(|| format!("rot_bytes {rot_bytes}: 4 or 6"))?;
    let len = (q.iter().map(|c| c * c).sum::<f32>()).sqrt();
    if !len.is_finite() || len < 1e-6 {
        return Err("a rotation is not a quaternion (zero length or not finite)".into());
    }
    let mut big = 0;
    for i in 1..4 {
        if q[i].abs() > q[big].abs() {
            big = i;
        }
    }
    let sign = if q[big] < 0.0 { -1.0 } else { 1.0 };
    let max = (1u64 << bits) - 1;
    let mut out = big as u64;
    let mut shift = 2;
    for (i, c) in q.iter().enumerate() {
        if i == big {
            continue;
        }
        let v = (c * sign / len / HALF_SQRT2).clamp(-1.0, 1.0);
        let k = (((v + 1.0) * 0.5) * max as f32).round() as u64;
        out |= k.min(max) << shift;
        shift += bits;
    }
    Ok(out)
}

/// The rotation [`encode_rot`] wrote (unit length).
pub fn decode_rot(bits_in: u64, rot_bytes: u8) -> Result<[f32; 4], String> {
    let bits = rot_bits(rot_bytes).ok_or_else(|| format!("rot_bytes {rot_bytes}: 4 or 6"))?;
    let max = (1u64 << bits) - 1;
    let big = (bits_in & 3) as usize;
    let mut q = [0.0f32; 4];
    let mut shift = 2;
    let mut sum = 0.0;
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
    Ok(normalize(q))
}

/// `v` in `min .. min + extent` as 16 bits.
pub fn quant_u16(v: f32, min: f32, extent: f32) -> u16 {
    if extent <= 0.0 || !extent.is_finite() {
        return 0;
    }
    (((v - min) / extent).clamp(0.0, 1.0) * 65535.0).round() as u16
}

pub fn dequant_u16(q: u16, min: f32, extent: f32) -> f32 {
    min + extent * f32::from(q) / 65535.0
}

pub fn normalize(q: [f32; 4]) -> [f32; 4] {
    let l = (q.iter().map(|c| c * c).sum::<f32>()).sqrt();
    if l > 0.0 && l.is_finite() { q.map(|c| c / l) } else { [0.0, 0.0, 0.0, 1.0] }
}

/// The decoder's rotation blend: nlerp on the shorter arc.
pub fn nlerp(a: [f32; 4], b: [f32; 4], u: f32) -> [f32; 4] {
    let dot: f32 = (0..4).map(|i| a[i] * b[i]).sum();
    let s = if dot < 0.0 { -1.0 } else { 1.0 };
    normalize([0, 1, 2, 3].map(|i| a[i] + (b[i] * s - a[i]) * u))
}

pub fn lerp(a: [f32; 4], b: [f32; 4], u: f32) -> [f32; 4] {
    [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * u)
}

/// The value between two keys, as the decoder blends it.
pub fn blend(channel: u8, a: [f32; 4], b: [f32; 4], u: f32) -> [f32; 4] {
    if channel == ROTATION { nlerp(a, b, u) } else { lerp(a, b, u) }
}

impl Track {
    /// The value at frame position `x` (fractional frames, clamped to the keys).
    pub fn sample_frame(&self, x: f32) -> [f32; 4] {
        let (Some(first), Some(&last)) = (self.values.first(), self.keys.last()) else { return [0.0, 0.0, 0.0, 1.0] };
        if self.keys.len() == 1 || x <= 0.0 {
            return *first;
        }
        let x = x.min(f32::from(last));
        // The last key at or before x (keys start at 0, so there is one).
        let k = self.keys.partition_point(|&f| f32::from(f) <= x).saturating_sub(1);
        if k + 1 >= self.keys.len() {
            return self.values[self.values.len() - 1];
        }
        let (f0, f1) = (f32::from(self.keys[k]), f32::from(self.keys[k + 1]));
        blend(self.channel, self.values[k], self.values[k + 1], (x - f0) / (f1 - f0))
    }

    /// The value at `seconds` into the clip.
    pub fn sample(&self, seconds: f32) -> [f32; 4] {
        self.sample_frame(seconds * RATE as f32)
    }
}

struct Rd<'a> {
    b: &'a [u8],
    at: usize,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| format!("clip block: data runs past the end (at byte {}, {n} more needed, {} in the block)", self.at, self.b.len()))?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn f32(&mut self) -> Result<f32, String> {
        let v = f32::from_bits(self.u32()?);
        if v.is_finite() { Ok(v) } else { Err(format!("clip block: a number that is not finite at byte {}", self.at - 4)) }
    }
    fn uint(&mut self, n: usize) -> Result<u64, String> {
        Ok(self.take(n)?.iter().rev().fold(0u64, |a, &x| (a << 8) | u64::from(x)))
    }
}

/// The clip offsets of a block, after checking its header.
pub fn offsets(block: &[u8]) -> Result<Vec<usize>, String> {
    let mut r = Rd { b: block, at: 0 };
    if r.take(4)? != MAGIC {
        return Err("clip block: no EVC1 magic".into());
    }
    let count = r.u16()?;
    if r.u16()? != 0 {
        return Err("clip block: reserved header bytes are not 0".into());
    }
    let mut out = Vec::new();
    for c in 0..count {
        let o = r.u32()? as usize;
        if !o.is_multiple_of(4) || o >= block.len() || o < 8 + 4 * usize::from(count) {
            return Err(format!("clip block: clip {c} offset {o} is not a 4-byte aligned place after the header"));
        }
        out.push(o);
    }
    Ok(out)
}

/// Every clip of a block, decoded and checked.
pub fn parse(block: &[u8]) -> Result<Vec<Clip>, String> {
    offsets(block)?.into_iter().enumerate().map(|(c, o)| parse_clip(&block[o..]).map_err(|e| format!("clip {c}: {e}"))).collect()
}

/// One clip from its first byte.
pub fn parse_clip(b: &[u8]) -> Result<Clip, String> {
    let mut r = Rd { b, at: 0 };
    let frames = r.u16()?;
    let n = r.u16()?;
    let rot_bytes = r.u8()?;
    let ib = r.u8()?;
    if r.u16()? != 0 {
        return Err("reserved clip header bytes are not 0".into());
    }
    if frames == 0 {
        return Err("a clip of 0 frames".into());
    }
    rot_bits(rot_bytes).ok_or_else(|| format!("rot_bytes {rot_bytes} (4 or 6)"))?;
    if ib != index_bytes(frames) {
        return Err(format!("index_bytes {ib} for {frames} frames (want {})", index_bytes(frames)));
    }
    let mut heads = Vec::new();
    for _ in 0..n {
        let node = r.u16()?;
        let keys = r.u16()?;
        let channel = r.u8()?;
        if r.u8()? != 0 {
            return Err("reserved track header byte is not 0".into());
        }
        if channel > SCALE {
            return Err(format!("track channel {channel} (0, 1 or 2)"));
        }
        if keys == 0 || keys > frames {
            return Err(format!("node {node} channel {channel}: {keys} keys in a clip of {frames} frames"));
        }
        if let Some(&(pn, _, pc)) = heads.last()
            && (pn, pc) >= (node, channel)
        {
            return Err(format!("tracks out of order or repeated at node {node} channel {channel}"));
        }
        heads.push((node, keys, channel));
    }
    let mut tracks = Vec::new();
    for (node, keys, channel) in heads {
        let start = r.at;
        let mut t = Track { node, channel, keys: Vec::new(), values: Vec::new(), bytes: 0 };
        if keys == 1 {
            t.keys.push(0);
            let v = if channel == ROTATION { normalize([r.f32()?, r.f32()?, r.f32()?, r.f32()?]) } else { [r.f32()?, r.f32()?, r.f32()?, 0.0] };
            t.values.push(v);
        } else {
            let range = if channel == ROTATION { None } else { Some(([r.f32()?, r.f32()?, r.f32()?], [r.f32()?, r.f32()?, r.f32()?])) };
            for k in 0..keys {
                let f = r.uint(usize::from(ib))? as u16;
                let ok = match t.keys.last() {
                    None => f == 0,
                    Some(&p) => f > p,
                };
                if !ok || (k + 1 == keys && f != frames - 1) {
                    return Err(format!("node {node} channel {channel}: key frames must run 0 .. {} strictly increasing (key {k} is {f})", frames - 1));
                }
                t.keys.push(f);
            }
            for _ in 0..keys {
                let v = match range {
                    None => decode_rot(r.uint(usize::from(rot_bytes))?, rot_bytes)?,
                    Some((min, ext)) => {
                        let (a, b2, c) = (r.u16()?, r.u16()?, r.u16()?);
                        [dequant_u16(a, min[0], ext[0]), dequant_u16(b2, min[1], ext[1]), dequant_u16(c, min[2], ext[2]), 0.0]
                    }
                };
                t.values.push(v);
            }
        }
        t.bytes = r.at - start;
        tracks.push(t);
    }
    Ok(Clip { frames, rot_bytes, index_bytes: ib, tracks, bytes: r.at })
}

/// The bytes of one clip (unpadded).
pub fn write_clip(c: &ClipIn) -> Result<Vec<u8>, String> {
    rot_bits(c.rot_bytes).ok_or_else(|| format!("rot_bytes {} (4 or 6)", c.rot_bytes))?;
    if c.frames == 0 {
        return Err("a clip of 0 frames".into());
    }
    let ib = index_bytes(c.frames);
    let n = u16::try_from(c.tracks.len()).map_err(|_| "over 65535 tracks in one clip")?;
    let mut o = Vec::new();
    o.extend_from_slice(&c.frames.to_le_bytes());
    o.extend_from_slice(&n.to_le_bytes());
    o.extend_from_slice(&[c.rot_bytes, ib, 0, 0]);
    for t in &c.tracks {
        let keys = u16::try_from(t.keys.len()).map_err(|_| "over 65535 keys in one track")?;
        if keys == 0 || keys != t.values.len() as u16 {
            return Err(format!("node {} channel {}: {} keys, {} values", t.node, t.channel, t.keys.len(), t.values.len()));
        }
        o.extend_from_slice(&t.node.to_le_bytes());
        o.extend_from_slice(&keys.to_le_bytes());
        o.extend_from_slice(&[t.channel, 0]);
    }
    let f32s = |o: &mut Vec<u8>, v: &[f32]| v.iter().for_each(|x| o.extend_from_slice(&x.to_le_bytes()));
    for t in &c.tracks {
        if t.keys.len() == 1 {
            f32s(&mut o, if t.channel == ROTATION { &t.values[0][..] } else { &t.values[0][..3] });
            continue;
        }
        let range = if t.channel == ROTATION {
            None
        } else {
            let (min, ext) = t.range.ok_or_else(|| format!("node {} channel {}: a keyed translation or scale needs its range", t.node, t.channel))?;
            f32s(&mut o, &min);
            f32s(&mut o, &ext);
            Some((min, ext))
        };
        for &k in &t.keys {
            o.extend_from_slice(&k.to_le_bytes()[..usize::from(ib)]);
        }
        for v in &t.values {
            match range {
                None => o.extend_from_slice(&encode_rot(*v, c.rot_bytes)?.to_le_bytes()[..usize::from(c.rot_bytes)]),
                Some((min, ext)) => (0..3).for_each(|i| o.extend_from_slice(&quant_u16(v[i], min[i], ext[i]).to_le_bytes())),
            }
        }
    }
    Ok(o)
}

/// A whole block from already-written clips (see [`write_clip`]).
pub fn write_block(clips: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let n = u16::try_from(clips.len()).map_err(|_| "over 65535 clips")?;
    let mut o = Vec::new();
    o.extend_from_slice(&MAGIC);
    o.extend_from_slice(&n.to_le_bytes());
    o.extend_from_slice(&[0, 0]);
    let at = 8 + 4 * clips.len();
    let mut body = Vec::new();
    for c in clips {
        while !(at + body.len()).is_multiple_of(4) {
            body.push(0);
        }
        o.extend_from_slice(&u32::try_from(at + body.len()).map_err(|_| "clip block over 4 GB")?.to_le_bytes());
        body.extend_from_slice(c);
    }
    o.extend_from_slice(&body);
    Ok(o)
}

/// The quantisation range of a translation or scale track's values.
pub fn range_of(values: &[[f32; 4]]) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for v in values {
        for i in 0..3 {
            lo[i] = lo[i].min(v[i]);
            hi[i] = hi[i].max(v[i]);
        }
    }
    (lo, [0, 1, 2].map(|i| (hi[i] - lo[i]).max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(x: f32, y: f32, z: f32, w: f32) -> [f32; 4] {
        normalize([x, y, z, w])
    }

    #[test]
    fn rotations_round_trip_in_both_widths() {
        for rb in [4u8, 6] {
            let worst = if rb == 4 { 2.0e-3 } else { 7.0e-5 };
            for a in [q(0.1, 0.2, 0.3, 0.9), q(-0.7, 0.1, 0.1, 0.1), q(0.0, 0.0, -1.0, 0.0), q(0.5, 0.5, 0.5, 0.5), [0.0, 0.0, 0.0, 1.0]] {
                let b = decode_rot(encode_rot(a, rb).unwrap(), rb).unwrap();
                let dot: f32 = (0..4).map(|i| a[i] * b[i]).sum();
                assert!(1.0 - dot.abs() < worst, "{rb} bytes: {a:?} -> {b:?} (dot {dot})");
            }
        }
    }

    #[test]
    fn a_written_clip_reads_back() {
        let c = ClipIn {
            frames: 31,
            rot_bytes: 6,
            tracks: vec![
                TrackIn { node: 0, channel: TRANSLATION, keys: vec![0, 15, 30], values: vec![[0.0, 1.0, 0.0, 0.0], [0.0, 1.2, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]], range: Some(([0.0, 1.0, 0.0], [0.0, 0.2, 0.0])) },
                TrackIn { node: 1, channel: ROTATION, keys: vec![0], values: vec![q(0.0, 0.0, 0.2, 1.0)], range: None },
                TrackIn { node: 1, channel: SCALE, keys: vec![0, 30], values: vec![[1.0; 4], [2.0; 4]], range: Some(([1.0; 3], [1.0; 3])) },
            ],
        };
        let block = write_block(&[write_clip(&c).unwrap(), write_clip(&ClipIn { frames: 1, rot_bytes: 4, tracks: Vec::new() }).unwrap()]).unwrap();
        let clips = parse(&block).unwrap();
        assert_eq!(clips.len(), 2);
        let a = &clips[0];
        assert_eq!((a.frames, a.tracks.len(), a.duration()), (31, 3, 1.0));
        assert!((a.tracks[0].sample(0.25)[1] - 1.1).abs() < 1e-4, "{:?}", a.tracks[0].sample(0.25));
        assert!((a.tracks[2].sample_frame(30.0)[0] - 2.0).abs() < 1e-4);
        assert_eq!(a.tracks[1].bytes, 16);
        assert_eq!(clips[1].tracks.len(), 0);
    }

    #[test]
    fn bad_blocks_are_errors_not_panics() {
        assert!(parse(b"").is_err());
        assert!(parse(b"EVC0\0\0\0\0").is_err());
        assert!(parse(b"EVC1\x01\0\0\0\x0c\0\0\0").is_err());
        let good = write_block(&[write_clip(&ClipIn { frames: 3, rot_bytes: 4, tracks: vec![TrackIn { node: 2, channel: ROTATION, keys: vec![0, 2], values: vec![[0.0, 0.0, 0.0, 1.0]; 2], range: None }] }).unwrap()]).unwrap();
        assert!(parse(&good).is_ok());
        for cut in 0..good.len() {
            assert!(parse(&good[..cut]).is_err(), "cut at {cut}");
        }
        for i in 8..good.len() {
            let mut b = good.clone();
            b[i] ^= 0xff;
            let _ = parse(&b);
        }
    }
}
