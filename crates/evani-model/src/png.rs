//! Our own PNG writer and reader for 8-bit greyscale images, using stored
//! (uncompressed) deflate blocks: valid PNG without a compression library
//! (decision D3). CRC-32 and Adler-32 are computed from their published
//! definitions. The reader accepts the files this writer makes (filter 0,
//! stored blocks), which is what M1 and M2 read (docs/MODEL_API.md 1.7).

fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (n, e) in t.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    t
}

pub fn crc32(data: &[u8]) -> u32 {
    let t = crc_table();
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = Vec::with_capacity(4 + data.len());
    body.extend_from_slice(ty);
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// Greyscale image, row-major, top row first.
#[derive(Clone, Debug, PartialEq)]
pub struct Grey {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Grey {
    pub fn new(w: usize, h: usize, fill: u8) -> Self {
        Grey { w, h, px: vec![fill; w * h] }
    }
    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.px[y * self.w + x]
    }
    pub fn set(&mut self, x: usize, y: usize, v: u8) {
        if x < self.w && y < self.h {
            self.px[y * self.w + x] = v;
        }
    }
    /// Copy `o` into this image at (x, y).
    pub fn blit(&mut self, o: &Grey, x: usize, y: usize) {
        for j in 0..o.h {
            for i in 0..o.w {
                self.set(x + i, y + j, o.get(i, j));
            }
        }
    }
}

pub fn write(img: &Grey) -> Vec<u8> {
    encode(img.w, img.h, 1, &img.px)
}

/// RGB image, 3 bytes per pixel, row-major.
pub fn write_rgb(w: usize, h: usize, px: &[u8]) -> Vec<u8> {
    encode(w, h, 3, px)
}

fn encode(w: usize, h: usize, ch: usize, px: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w * ch + 1) * h);
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&px[y * w * ch..(y + 1) * w * ch]);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        let l = b.len() as u16;
        z.extend_from_slice(&l.to_le_bytes());
        z.extend_from_slice(&(!l).to_le_bytes());
        z.extend_from_slice(b);
    }
    if blocks.is_empty() {
        z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, if ch == 3 { 2 } else { 0 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Read a greyscale PNG written with stored blocks and filter 0.
pub fn read(bytes: &[u8]) -> Result<Grey, String> {
    if bytes.get(0..8) != Some(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Err("not a PNG".into());
    }
    let (mut w, mut h) = (0usize, 0usize);
    let mut z = Vec::new();
    let mut o = 8;
    while o + 8 <= bytes.len() {
        let l = u32::from_be_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as usize;
        let ty = &bytes[o + 4..o + 8];
        let data = bytes.get(o + 8..o + 8 + l).ok_or("truncated chunk")?;
        match ty {
            b"IHDR" => {
                w = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
                h = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
                if data[8] != 8 || data[9] != 0 {
                    return Err("only 8-bit greyscale masks are read (made by our reference tool)".into());
                }
            }
            b"IDAT" => z.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        o += 12 + l;
    }
    let mut raw = Vec::new();
    let mut p = 2;
    loop {
        let hdr = *z.get(p).ok_or("truncated deflate")?;
        if hdr & 0b110 != 0 {
            return Err("compressed deflate blocks are not read in M1 (only our own stored masks)".into());
        }
        let l = u16::from_le_bytes([*z.get(p + 1).ok_or("truncated")?, *z.get(p + 2).ok_or("truncated")?]) as usize;
        raw.extend_from_slice(z.get(p + 5..p + 5 + l).ok_or("truncated block")?);
        p += 5 + l;
        if hdr & 1 == 1 {
            break;
        }
    }
    if raw.len() != (w + 1) * h {
        return Err("size mismatch".into());
    }
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        if raw[y * (w + 1)] != 0 {
            return Err("only filter 0 rows are read".into());
        }
        px.extend_from_slice(&raw[y * (w + 1) + 1..(y + 1) * (w + 1)]);
    }
    Ok(Grey { w, h, px })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_published_values() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn round_trip_including_multi_block() {
        let mut g = Grey::new(300, 260, 0);
        for y in 0..g.h {
            for x in 0..g.w {
                g.set(x, y, ((x * 7 + y * 3) % 256) as u8);
            }
        }
        let b = write(&g);
        assert_eq!(read(&b).unwrap(), g);
        assert_eq!(write(&g), b);
    }
}
