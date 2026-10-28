//! Small image tools for live captures, with no dependencies: area
//! downscale (RGBA to RGB), a PNG encoder (per-row filters, zlib with one
//! fixed-Huffman deflate block and LZ77 hash chains), base64, and a
//! per-pixel difference measure for `compare.ab`.
//!
//! Dev only: evani-live never ships in a game, so none of this reaches a
//! game's download. The renderer hands back raw RGBA; everything here runs
//! on the page's main thread, so it is kept simple and allocation-light.

/// An 8-bit RGB image, rows top first, `w * h * 3` bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

/// Downscale tightly packed RGBA (`w * h * 4`) to RGB `out_w` wide
/// (never wider than the source), height following the aspect ratio. Each
/// output pixel is the average of the source pixels it covers.
pub fn downscale_rgba(rgba: &[u8], w: u32, h: u32, out_w: u32) -> Rgb {
    let (w, h) = (w.max(1), h.max(1));
    let ow = out_w.clamp(1, w);
    let oh = ((u64::from(h) * u64::from(ow) + u64::from(w) / 2) / u64::from(w)).max(1) as u32;
    let mut px = Vec::with_capacity((ow * oh * 3) as usize);
    let (wu, hu, owu, ohu) = (w as usize, h as usize, ow as usize, oh as usize);
    for oy in 0..ohu {
        let y0 = oy * hu / ohu;
        let y1 = ((oy + 1) * hu / ohu).max(y0 + 1).min(hu);
        for ox in 0..owu {
            let x0 = ox * wu / owu;
            let x1 = ((ox + 1) * wu / owu).max(x0 + 1).min(wu);
            let mut sum = [0u32; 3];
            for y in y0..y1 {
                let row = y * wu * 4;
                for x in x0..x1 {
                    let i = row + x * 4;
                    if let Some(p) = rgba.get(i..i + 3) {
                        sum[0] += u32::from(p[0]);
                        sum[1] += u32::from(p[1]);
                        sum[2] += u32::from(p[2]);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            for s in sum {
                px.push(((s + n / 2) / n) as u8);
            }
        }
    }
    Rgb { w: ow, h: oh, px }
}

// ---- PNG ------------------------------------------------------------------------

const CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
}

pub fn crc32(parts: &[&[u8]]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for p in parts {
        for &b in *p {
            c = CRC_TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
        }
    }
    c ^ 0xFFFF_FFFF
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&[kind, data]).to_be_bytes());
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Filter every row with whichever of the 5 PNG filters gives the smallest
/// sum of absolute (signed) values; returns the filtered scanlines.
fn filter_rows(img: &Rgb) -> Vec<u8> {
    const BPP: usize = 3;
    let stride = img.w as usize * BPP;
    let mut out = Vec::with_capacity((stride + 1) * img.h as usize);
    let zero = vec![0u8; stride];
    let mut cand = [vec![0u8; stride], vec![0u8; stride], vec![0u8; stride], vec![0u8; stride], vec![0u8; stride]];
    for y in 0..img.h as usize {
        let row = &img.px[y * stride..(y + 1) * stride];
        let up = if y == 0 { &zero[..] } else { &img.px[(y - 1) * stride..y * stride] };
        for i in 0..stride {
            let a = if i >= BPP { row[i - BPP] } else { 0 };
            let b = up[i];
            let c = if i >= BPP { up[i - BPP] } else { 0 };
            let x = row[i];
            cand[0][i] = x;
            cand[1][i] = x.wrapping_sub(a);
            cand[2][i] = x.wrapping_sub(b);
            cand[3][i] = x.wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8);
            cand[4][i] = x.wrapping_sub(paeth(a, b, c));
        }
        let score = |v: &[u8]| v.iter().map(|&b| u32::from((b as i8).unsigned_abs())).sum::<u32>();
        let best = (0..5).min_by_key(|&f| score(&cand[f])).unwrap_or(0);
        out.push(best as u8);
        out.extend_from_slice(&cand[best]);
    }
    out
}

/// Encode an RGB image as PNG.
pub fn encode_png(img: &Rgb) -> Vec<u8> {
    let raw = filter_rows(img);
    let mut z = Vec::with_capacity(raw.len() / 2 + 64);
    z.extend_from_slice(&[0x78, 0x01]);
    deflate_fixed(&raw, &mut z);
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut out = Vec::with_capacity(z.len() + 64);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = [0u8; 13];
    ihdr[..4].copy_from_slice(&img.w.to_be_bytes());
    ihdr[4..8].copy_from_slice(&img.h.to_be_bytes());
    ihdr[8] = 8; // bit depth
    ihdr[9] = 2; // colour type: RGB
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

// ---- deflate (one fixed-Huffman block) ------------------------------------------

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

const WINDOW: usize = 32 * 1024;
const HASH_BITS: u32 = 15;
const MAX_CHAIN: usize = 32;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const NONE: u32 = u32::MAX;

struct Bits<'a> {
    out: &'a mut Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits<'_> {
    /// Append `n` bits of `v`, least significant first.
    fn put(&mut self, v: u32, n: u32) {
        self.acc |= u64::from(v) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// Append a Huffman code (stored most significant bit first).
    fn code(&mut self, code: u32, len: u32) {
        self.put(code.reverse_bits() >> (32 - len), len);
    }

    fn flush(&mut self) {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.acc = 0;
        self.n = 0;
    }

    fn literal(&mut self, sym: u32) {
        match sym {
            0..=143 => self.code(0x30 + sym, 8),
            144..=255 => self.code(0x190 + sym - 144, 9),
            256..=279 => self.code(sym - 256, 7),
            _ => self.code(0xC0 + sym - 280, 8),
        }
    }

    fn matched(&mut self, len: usize, dist: usize) {
        let li = LEN_BASE.iter().rposition(|&b| usize::from(b) <= len).unwrap_or(0);
        self.literal(257 + li as u32);
        if LEN_EXTRA[li] > 0 {
            self.put((len - usize::from(LEN_BASE[li])) as u32, u32::from(LEN_EXTRA[li]));
        }
        let di = DIST_BASE.iter().rposition(|&b| usize::from(b) <= dist).unwrap_or(0);
        self.code(di as u32, 5);
        if DIST_EXTRA[di] > 0 {
            self.put((dist - usize::from(DIST_BASE[di])) as u32, u32::from(DIST_EXTRA[di]));
        }
    }
}

fn hash3(d: &[u8], i: usize) -> usize {
    let v = u32::from(d[i]) << 16 | u32::from(d[i + 1]) << 8 | u32::from(d[i + 2]);
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

/// Compress `data` as one final fixed-Huffman deflate block into `out`.
pub fn deflate_fixed(data: &[u8], out: &mut Vec<u8>) {
    let mut bits = Bits { out, acc: 0, n: 0 };
    bits.put(1, 1); // BFINAL
    bits.put(1, 2); // BTYPE = 01, fixed Huffman
    let n = data.len();
    let mut head = vec![NONE; 1 << HASH_BITS];
    let mut prev = vec![NONE; WINDOW];
    let insert = |head: &mut [u32], prev: &mut [u32], i: usize| {
        if i + MIN_MATCH <= n {
            let h = hash3(data, i);
            prev[i % WINDOW] = head[h];
            head[h] = i as u32;
        }
    };
    let mut i = 0;
    while i < n {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i + MIN_MATCH <= n {
            let max = (n - i).min(MAX_MATCH);
            let mut cand = head[hash3(data, i)];
            let mut chain = 0;
            while cand != NONE && chain < MAX_CHAIN {
                let c = cand as usize;
                if i - c > WINDOW - 1 || c >= i {
                    break;
                }
                let mut l = 0;
                while l < max && data[c + l] == data[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - c;
                    if l == max {
                        break;
                    }
                }
                let next = prev[c % WINDOW];
                if next != NONE && next as usize >= c {
                    break;
                }
                cand = next;
                chain += 1;
            }
        }
        if best_len >= MIN_MATCH {
            bits.matched(best_len, best_dist);
            for k in i..i + best_len {
                insert(&mut head, &mut prev, k);
            }
            i += best_len;
        } else {
            bits.literal(u32::from(data[i]));
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    bits.literal(256); // end of block
    bits.flush();
}

// ---- base64 ---------------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn base64(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let v = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for k in 0..4 {
            if k <= c.len() {
                s.push(char::from(B64[((v >> (18 - 6 * k)) & 63) as usize]));
            } else {
                s.push('=');
            }
        }
    }
    s
}

// ---- difference -----------------------------------------------------------------

/// Per-pixel difference of two same-size RGB images (channel values 0-255).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Diff {
    /// Mean absolute channel difference.
    pub mean_abs: f64,
    /// Root mean square channel difference.
    pub rmse: f64,
    /// Largest channel difference.
    pub max: u8,
    /// Percent of pixels where any channel differs by more than
    /// [`CHANGED_THRESHOLD`].
    pub changed_pct: f64,
    /// Peak signal-to-noise ratio in dB; `None` for identical images.
    pub psnr_db: Option<f64>,
}

/// A pixel counts as changed when a channel moves by more than this.
pub const CHANGED_THRESHOLD: u8 = 8;

/// `None` when the sizes differ.
pub fn diff(a: &Rgb, b: &Rgb) -> Option<Diff> {
    if a.w != b.w || a.h != b.h || a.px.len() != b.px.len() || a.px.is_empty() {
        return None;
    }
    let (mut sum, mut sq, mut max, mut changed) = (0u64, 0u64, 0u8, 0u64);
    for (pa, pb) in a.px.as_chunks::<3>().0.iter().zip(b.px.as_chunks::<3>().0) {
        let mut pix_max = 0u8;
        for k in 0..3 {
            let d = pa[k].abs_diff(pb[k]);
            sum += u64::from(d);
            sq += u64::from(d) * u64::from(d);
            pix_max = pix_max.max(d);
        }
        max = max.max(pix_max);
        if pix_max > CHANGED_THRESHOLD {
            changed += 1;
        }
    }
    let n = a.px.len() as f64;
    let mse = sq as f64 / n;
    Some(Diff {
        mean_abs: sum as f64 / n,
        rmse: mse.sqrt(),
        max,
        changed_pct: changed as f64 * 100.0 / (n / 3.0),
        psnr_db: (mse > 0.0).then(|| 10.0 * (255.0 * 255.0 / mse).log10()),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn inflate_zlib(z: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(z)
            .read_to_end(&mut out)
            .expect("valid zlib stream");
        out
    }

    /// Decode our own PNG (IHDR + one IDAT, RGB8) back to pixels.
    fn decode_png(png: &[u8]) -> Rgb {
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        let mut at = 8;
        let (mut w, mut h, mut idat) = (0, 0, Vec::new());
        while at < png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let kind = &png[at + 4..at + 8];
            let data = &png[at + 8..at + 8 + len];
            let crc = u32::from_be_bytes(png[at + 8 + len..at + 12 + len].try_into().unwrap());
            assert_eq!(crc, crc32(&[kind, data]), "chunk crc");
            match kind {
                b"IHDR" => {
                    w = u32::from_be_bytes(data[..4].try_into().unwrap());
                    h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                    assert_eq!(&data[8..], &[8, 2, 0, 0, 0]);
                }
                b"IDAT" => idat.extend_from_slice(data),
                _ => {}
            }
            at += 12 + len;
        }
        let raw = inflate_zlib(&idat);
        let stride = w as usize * 3;
        assert_eq!(raw.len(), (stride + 1) * h as usize);
        let mut px = vec![0u8; stride * h as usize];
        for y in 0..h as usize {
            let f = raw[y * (stride + 1)];
            for i in 0..stride {
                let x = raw[y * (stride + 1) + 1 + i];
                let a = if i >= 3 { px[y * stride + i - 3] } else { 0 };
                let b = if y > 0 { px[(y - 1) * stride + i] } else { 0 };
                let c = if i >= 3 && y > 0 { px[(y - 1) * stride + i - 3] } else { 0 };
                px[y * stride + i] = match f {
                    0 => x,
                    1 => x.wrapping_add(a),
                    2 => x.wrapping_add(b),
                    3 => x.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                    4 => x.wrapping_add(paeth(a, b, c)),
                    _ => panic!("bad filter {f}"),
                };
            }
        }
        Rgb { w, h, px }
    }

    fn noise(len: usize, seed: u32) -> Vec<u8> {
        let mut x = seed.max(1);
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect()
    }

    #[test]
    fn deflate_round_trips_through_flate2() {
        let mut long = b"abcabcabcabcabcabcabcabcabcabcabcabcabcabc".repeat(400);
        long.extend(noise(70_000, 7));
        long.extend(vec![0u8; 40_000]);
        for data in [Vec::new(), vec![42u8], b"ab".to_vec(), long] {
            let mut z = vec![0x78, 0x01];
            deflate_fixed(&data, &mut z);
            z.extend_from_slice(&adler32(&data).to_be_bytes());
            assert_eq!(inflate_zlib(&z), data, "len {}", data.len());
        }
    }

    #[test]
    fn repetitive_data_compresses() {
        let data = vec![7u8; 100_000];
        let mut z = Vec::new();
        deflate_fixed(&data, &mut z);
        assert!(z.len() < 1000, "{}", z.len());
    }

    #[test]
    fn png_round_trips_and_is_small_for_smooth_images() {
        let (w, h) = (97u32, 61u32);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[(x * 2) as u8, (y * 4) as u8, ((x + y) % 256) as u8]);
            }
        }
        let img = Rgb { w, h, px };
        let png = encode_png(&img);
        assert_eq!(decode_png(&png), img);
        assert!(png.len() < img.px.len() / 4, "{} of {}", png.len(), img.px.len());

        let noisy = Rgb { w: 33, h: 17, px: noise(33 * 17 * 3, 99) };
        assert_eq!(decode_png(&encode_png(&noisy)), noisy);
    }

    #[test]
    fn crc_and_adler_match_known_values() {
        assert_eq!(crc32(&[b"123456789"]), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn base64_matches_rfc4648() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn downscale_averages_areas_and_keeps_aspect() {
        // 4x2 RGBA: left half black, right half white.
        let mut rgba = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                let v = if x < 2 { 0 } else { 255 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let out = downscale_rgba(&rgba, 4, 2, 2);
        assert_eq!((out.w, out.h), (2, 1));
        assert_eq!(out.px, vec![0, 0, 0, 255, 255, 255]);
        let half = downscale_rgba(&rgba, 4, 2, 1);
        assert_eq!(half.px, vec![128, 128, 128]);
        // Never upscales.
        let same = downscale_rgba(&rgba, 4, 2, 1000);
        assert_eq!((same.w, same.h), (4, 2));
        // 1920x1080 at 480 keeps 16:9.
        let big = vec![10u8; 1920 * 1080 * 4];
        let s = downscale_rgba(&big, 1920, 1080, 480);
        assert_eq!((s.w, s.h), (480, 270));
        assert!(s.px.iter().all(|&v| v == 10));
    }

    #[test]
    fn diff_measures() {
        let a = Rgb { w: 2, h: 1, px: vec![10, 10, 10, 200, 200, 200] };
        let same = diff(&a, &a).unwrap();
        assert_eq!(same.mean_abs, 0.0);
        assert_eq!(same.changed_pct, 0.0);
        assert!(same.psnr_db.is_none());
        // Second pixel moves by 20 on one channel; first by 4 (below threshold).
        let b = Rgb { w: 2, h: 1, px: vec![14, 10, 10, 220, 200, 200] };
        let d = diff(&a, &b).unwrap();
        assert_eq!(d.max, 20);
        assert!((d.mean_abs - 24.0 / 6.0).abs() < 1e-9);
        assert!((d.rmse - ((16.0 + 400.0) / 6.0f64).sqrt()).abs() < 1e-9);
        assert_eq!(d.changed_pct, 50.0);
        assert!(d.psnr_db.unwrap() > 20.0);
        let other = Rgb { w: 1, h: 2, px: a.px.clone() };
        assert!(diff(&a, &other).is_none());
    }
}
