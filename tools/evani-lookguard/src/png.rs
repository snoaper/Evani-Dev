//! A small PNG decoder for capture images: 8-bit RGB or RGBA, not
//! interlaced (what `capture.frame` writes, and what most tools save).
//! Alpha is dropped: the stats work on the final sRGB colour only.

use std::io::Read;

/// An 8-bit RGB image, rows top first, `w * h * 3` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
/// Larger images are refused (captures are at most 4096 wide).
const MAX_PIXELS: u64 = 4096 * 4096;

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
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

/// Standard base64 (with or without `=` padding) to bytes, for the
/// `png_base64` of a `capture.frame` result.
pub fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let s = s.trim().trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = val(c).ok_or("not base64")?;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// Decode a PNG to RGB.
pub fn decode(bytes: &[u8]) -> Result<Rgb, String> {
    if bytes.len() < 8 || bytes[..8] != SIGNATURE {
        return Err("not a PNG file".into());
    }
    let mut pos = 8;
    let mut header: Option<(u32, u32, u8)> = None;
    let mut idat = Vec::new();
    loop {
        if pos + 8 > bytes.len() {
            return Err("PNG ends before IEND".into());
        }
        let len = be32(&bytes[pos..]) as usize;
        let kind = &bytes[pos + 4..pos + 8];
        let data_start = pos + 8;
        let data_end = data_start.checked_add(len).ok_or("PNG chunk too long")?;
        if data_end + 4 > bytes.len() {
            return Err("PNG chunk runs past the end".into());
        }
        let data = &bytes[data_start..data_end];
        match kind {
            b"IHDR" => {
                if data.len() != 13 {
                    return Err("bad IHDR".into());
                }
                let (w, h) = (be32(data), be32(&data[4..]));
                let (depth, color, interlace) = (data[8], data[9], data[12]);
                if depth != 8 || !(color == 2 || color == 6) {
                    return Err(format!("only 8-bit RGB or RGBA PNGs are read (bit depth {depth}, colour type {color})"));
                }
                if interlace != 0 {
                    return Err("interlaced PNGs are not read".into());
                }
                if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
                    return Err(format!("PNG size {w}x{h} is out of range"));
                }
                header = Some((w, h, if color == 6 { 4 } else { 3 }));
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        pos = data_end + 4; // skip the CRC
    }
    let (w, h, bpp) = header.ok_or("PNG has no IHDR")?;
    let (wu, hu, bppu) = (w as usize, h as usize, usize::from(bpp));
    let stride = wu * bppu;
    let want = (stride + 1) * hu;
    let mut raw = Vec::with_capacity(want);
    flate2::read::ZlibDecoder::new(&idat[..])
        .take(want as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|e| format!("PNG data is not valid zlib: {e}"))?;
    if raw.len() != want {
        return Err(format!("PNG data is {} bytes, expected {want}", raw.len()));
    }
    let mut prev = vec![0u8; stride];
    let mut cur = vec![0u8; stride];
    let mut px = Vec::with_capacity(wu * hu * 3);
    for y in 0..hu {
        let row = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let filter = row[0];
        cur.copy_from_slice(&row[1..]);
        for i in 0..stride {
            let a = if i >= bppu { cur[i - bppu] } else { 0 };
            let b = prev[i];
            let c = if i >= bppu { prev[i - bppu] } else { 0 };
            let add = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => paeth(a, b, c),
                f => return Err(format!("PNG row {y} has unknown filter {f}")),
            };
            cur[i] = cur[i].wrapping_add(add);
        }
        for p in cur.chunks_exact(bppu) {
            px.extend_from_slice(&p[..3]);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    Ok(Rgb { w, h, px })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_live_encoder() {
        // evani-live's encoder uses every filter type across rows.
        let (w, h) = (37u32, 23u32);
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                px.extend_from_slice(&[(x * 7) as u8, (y * 11) as u8, ((x * y) % 256) as u8]);
            }
        }
        let img = evani_live::img::Rgb { w, h, px: px.clone() };
        let png = evani_live::img::encode_png(&img);
        let back = decode(&png).unwrap();
        assert_eq!((back.w, back.h), (w, h));
        assert_eq!(back.px, px);
        // And through the live API's base64, as capture.frame sends it.
        let b64 = evani_live::img::base64(&png);
        assert_eq!(base64_decode(&b64).unwrap(), png);
        assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
        assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
        assert_eq!(base64_decode("TQ==").unwrap(), b"M");
        assert!(base64_decode("TW*u").is_err());
    }

    #[test]
    fn rgba_drops_alpha_and_bad_input_fails() {
        // Hand-made 2x1 RGBA, filter 0, stored with flate2.
        use std::io::Write;
        let raw = [0u8, 10, 20, 30, 255, 40, 50, 60, 0];
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).unwrap();
        let idat = z.finish().unwrap();
        let mut png = SIGNATURE.to_vec();
        let mut chunk = |kind: &[u8], data: &[u8]| {
            png.extend_from_slice(&(data.len() as u32).to_be_bytes());
            png.extend_from_slice(kind);
            png.extend_from_slice(data);
            png.extend_from_slice(&[0, 0, 0, 0]);
        };
        chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        chunk(b"IDAT", &idat);
        chunk(b"IEND", &[]);
        let img = decode(&png).unwrap();
        assert_eq!(img.px, vec![10, 20, 30, 40, 50, 60]);
        assert!(decode(b"nope").is_err());
        assert!(decode(&png[..png.len() - 12]).is_err());
    }
}
