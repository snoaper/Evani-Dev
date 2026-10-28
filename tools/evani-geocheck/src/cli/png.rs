//! PNG read and write for the render checks (8-bit RGB or RGBA, not
//! interlaced: what `capture.frame` writes) through flate2.

use std::io::{Read, Write};

use evani_geocheck::image::Image;

fn crc32(data: &[&[u8]]) -> u32 {
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for part in data {
        for &b in *part {
            c = table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
        }
    }
    c ^ 0xFFFF_FFFF
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = ((p - i16::from(a)).abs(), (p - i16::from(b)).abs(), (p - i16::from(c)).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Decode a PNG into RGBA.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("not a PNG".into());
    }
    let mut pos = 8;
    let (mut w, mut h, mut ctype) = (0u32, 0u32, 0u8);
    let mut idat = Vec::new();
    while pos + 8 <= bytes.len() {
        let n = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap_or_default()) as usize;
        let kind = &bytes[pos + 4..pos + 8];
        let body = bytes.get(pos + 8..pos + 8 + n).ok_or("truncated PNG chunk")?;
        match kind {
            b"IHDR" => {
                w = u32::from_be_bytes(body[0..4].try_into().unwrap_or_default());
                h = u32::from_be_bytes(body[4..8].try_into().unwrap_or_default());
                let depth = body[8];
                ctype = body[9];
                if depth != 8 || !(ctype == 2 || ctype == 6) || body[12] != 0 {
                    return Err(format!("unsupported PNG (depth {depth}, colour type {ctype}, interlace {})", body[12]));
                }
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos += 12 + n;
    }
    if w == 0 || h == 0 {
        return Err("PNG without a size".into());
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .read_to_end(&mut raw)
        .map_err(|e| format!("PNG data: {e}"))?;
    let bpp = if ctype == 6 { 4 } else { 3 };
    let stride = w as usize * bpp;
    if raw.len() < (stride + 1) * h as usize {
        return Err("PNG data too short".into());
    }
    let mut cur = vec![0u8; stride];
    let mut prev = vec![0u8; stride];
    let mut img = Image::new(w, h);
    for y in 0..h as usize {
        let row = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let filter = row[0];
        for x in 0..stride {
            let a = if x >= bpp { cur[x - bpp] } else { 0 };
            let b = prev[x];
            let c = if x >= bpp { prev[x - bpp] } else { 0 };
            let v = row[1 + x];
            cur[x] = match filter {
                0 => v,
                1 => v.wrapping_add(a),
                2 => v.wrapping_add(b),
                3 => v.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => v.wrapping_add(paeth(a, b, c)),
                f => return Err(format!("bad PNG filter {f}")),
            };
        }
        for x in 0..w as usize {
            let p = &cur[x * bpp..x * bpp + bpp];
            img.set(x as u32, y as u32, [p[0], p[1], p[2], if bpp == 4 { p[3] } else { 255 }]);
        }
        std::mem::swap(&mut cur, &mut prev);
    }
    Ok(img)
}

/// Encode RGBA as a PNG (filter "up" per row, zlib).
pub fn encode(img: &Image) -> Vec<u8> {
    let stride = img.w as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * img.h as usize);
    for y in 0..img.h as usize {
        raw.push(2);
        let row = &img.data[y * stride..(y + 1) * stride];
        if y == 0 {
            raw.extend_from_slice(row);
        } else {
            let up = &img.data[(y - 1) * stride..y * stride];
            raw.extend(row.iter().zip(up).map(|(a, b)| a.wrapping_sub(*b)));
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    // Writing to a Vec cannot fail.
    let _ = z.write_all(&raw);
    let z = z.finish().unwrap_or_default();
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out.extend_from_slice(&crc32(&[kind, body]).to_be_bytes());
    };
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&img.w.to_be_bytes());
    ihdr.extend_from_slice(&img.h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut img = Image::new(5, 3);
        for y in 0..3 {
            for x in 0..5 {
                img.set(x, y, [(x * 40) as u8, (y * 70) as u8, 200 - (x * y) as u8, 255 - x as u8]);
            }
        }
        let back = decode(&encode(&img)).expect("decode");
        assert_eq!(back, img);
    }

    #[test]
    fn decodes_rgb_with_every_filter() {
        // 3 x 5 RGB, rows filtered with 0, 1, 2, 3, 4.
        let (w, h, bpp) = (3usize, 5usize, 3usize);
        let px: Vec<Vec<u8>> = (0..h).map(|y| (0..w * bpp).map(|i| (i * 37 + y * 91 + 13) as u8).collect()).collect();
        let mut raw = Vec::new();
        for (y, row) in px.iter().enumerate() {
            let f = y as u8;
            raw.push(f);
            for x in 0..w * bpp {
                let a = if x >= bpp { row[x - bpp] } else { 0 };
                let b = if y > 0 { px[y - 1][x] } else { 0 };
                let c = if y > 0 && x >= bpp { px[y - 1][x - bpp] } else { 0 };
                let pred = match f {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    _ => paeth(a, b, c),
                };
                raw.push(row[x].wrapping_sub(pred));
            }
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        z.write_all(&raw).unwrap();
        let z = z.finish().unwrap();
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, body) in [(*b"IHDR", [&(w as u32).to_be_bytes()[..], &(h as u32).to_be_bytes(), &[8, 2, 0, 0, 0]].concat()), (*b"IDAT", z), (*b"IEND", Vec::new())] {
            png.extend_from_slice(&(body.len() as u32).to_be_bytes());
            png.extend_from_slice(&kind);
            png.extend_from_slice(&body);
            png.extend_from_slice(&crc32(&[&kind, &body]).to_be_bytes());
        }
        let img = decode(&png).expect("decode");
        for (y, row) in px.iter().enumerate().take(h) {
            for x in 0..w {
                let p = &row[x * 3..x * 3 + 3];
                assert_eq!(img.px(x as u32, y as u32), [p[0], p[1], p[2], 255], "pixel {x},{y}");
            }
        }
    }

    #[test]
    fn crc_of_iend() {
        assert_eq!(crc32(&[b"IEND"]), 0xAE42_6082);
    }
}
