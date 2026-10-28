//! EXT_meshopt_compression decoder: the three bitstream modes (ATTRIBUTES v0,
//! TRIANGLES, INDICES) and the three filters (OCTAHEDRAL, QUATERNION,
//! EXPONENTIAL), scalar, no SIMD, no allocation beyond the caller's output.
//!
//! Written from the extension specification (Khronos glTF repository,
//! `extensions/2.0/Vendor/EXT_meshopt_compression/README.md`, Appendix A
//! "Bitstream" and Appendix B "Filters"). The decoder's shape (block by block,
//! byte column by byte column, 16-element groups; edge and vertex FIFOs for
//! triangles; the tail checks) follows meshoptimizer's scalar decoders
//! `decodeVertexBuffer` / `decodeIndexBuffer` / `decodeIndexSequence`, whose
//! licence is kept here:
//!
//! meshoptimizer, https://github.com/zeux/meshoptimizer
//! MIT License
//!
//! Copyright (c) 2016-2026 Arseny Kapoulkine
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.
//!
//! Every read is bounds-checked: a short, corrupt or hostile stream returns an
//! [`Error`], never a panic.

use crate::Error;

/// The compression mode of a compressed buffer view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Attributes,
    Triangles,
    Indices,
}

/// The filter applied after decoding (ATTRIBUTES mode only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    None,
    Octahedral,
    Quaternion,
    Exponential,
}

const BAD: Error = err!("M11", "meshopt: corrupt or truncated stream");

/// Decode one compressed buffer view into `out` (`count * stride` bytes),
/// checking the extension's validity rules first.
pub fn decode(out: &mut [u8], count: usize, stride: usize, mode: Mode, filter: Filter, src: &[u8]) -> Result<(), Error> {
    if count.checked_mul(stride) != Some(out.len()) {
        return Err(err!("M12", "meshopt: byteLength is not count * byteStride"));
    }
    match mode {
        Mode::Attributes => {
            if stride == 0 || stride > 256 || !stride.is_multiple_of(4) {
                return Err(err!("M13", "meshopt: bad ATTRIBUTES byteStride"));
            }
            decode_vertex_buffer(out, count, stride, src)?;
            match filter {
                Filter::None => Ok(()),
                Filter::Octahedral => filter_oct(out, stride),
                Filter::Quaternion => filter_quat(out, stride),
                Filter::Exponential => {
                    filter_exp(out);
                    Ok(())
                }
            }
        }
        Mode::Triangles | Mode::Indices => {
            if (stride != 2 && stride != 4) || filter != Filter::None {
                return Err(err!("M14", "meshopt: bad index byteStride or filter"));
            }
            if mode == Mode::Triangles { decode_triangles(out, count, stride, src) } else { decode_indices(out, count, stride, src) }
        }
    }
}

fn byte(src: &[u8], pos: &mut usize, end: usize) -> Result<u8, Error> {
    if *pos >= end {
        return Err(BAD);
    }
    let b = src.get(*pos).copied().ok_or(BAD)?;
    *pos += 1;
    Ok(b)
}

#[inline]
fn unzig8(v: u8) -> u8 {
    if v & 1 != 0 { !(v >> 1) } else { v >> 1 }
}

/// Mode 0 (attributes), bitstream version 0 (header `0xa0`).
fn decode_vertex_buffer(out: &mut [u8], count: usize, stride: usize, src: &[u8]) -> Result<(), Error> {
    match src.first() {
        Some(0xa0) => {}
        Some(h) if h & 0xf0 == 0xa0 => return Err(err!("M15", "meshopt: vertex codec version other than 0 (EXT allows only v0)")),
        _ => return Err(err!("M16", "meshopt: bad ATTRIBUTES header")),
    }
    let tail = stride.max(32);
    if src.len() < 1 + tail || (count / 64).saturating_mul(stride) > src.len() {
        return Err(BAD);
    }
    let end = src.len() - tail;
    let mut last = [0u8; 256];
    last[..stride].copy_from_slice(src.get(src.len() - stride..).ok_or(BAD)?);
    let block_max = ((8192 / stride) & !15).min(256);
    let mut deltas = [0u8; 256];
    let mut pos = 1;
    let mut start = 0;
    while start < count {
        let n = (count - start).min(block_max);
        let groups = n.div_ceil(16);
        for k in 0..stride {
            let header_pos = pos;
            pos += groups.div_ceil(4);
            if pos > end {
                return Err(BAD);
            }
            for g in 0..groups {
                let bits = (src[header_pos + g / 4] >> ((g % 4) * 2)) & 3;
                let d = &mut deltas[g * 16..g * 16 + 16];
                match bits {
                    0 => d.fill(0),
                    3 => {
                        d.copy_from_slice(src.get(pos..pos + 16).filter(|_| pos + 16 <= end).ok_or(BAD)?);
                        pos += 16;
                    }
                    _ => {
                        // 2-bit (bits 1) or 4-bit (bits 2) sentinel coding.
                        let w = bits as usize * 2;
                        let per = 8 / w;
                        let sentinel = (1u8 << w) - 1;
                        let mut extra = pos + 16 / per;
                        if extra > end {
                            return Err(BAD);
                        }
                        for (j, dj) in d.iter_mut().enumerate() {
                            let shift = 8 - w * (j % per + 1);
                            let v = (src[pos + j / per] >> shift) & sentinel;
                            *dj = if v == sentinel { byte(src, &mut extra, end)? } else { v };
                        }
                        pos = extra;
                    }
                }
            }
            let prev = &mut last[k];
            for (i, &dz) in deltas[..n].iter().enumerate() {
                *prev = prev.wrapping_add(unzig8(dz));
                out[(start + i) * stride + k] = *prev;
            }
        }
        start += n;
    }
    if pos != end {
        return Err(BAD);
    }
    Ok(())
}

fn write_index(out: &mut [u8], i: usize, stride: usize, v: u32) {
    if stride == 2 {
        out[i * 2..i * 2 + 2].copy_from_slice(&(v as u16).to_le_bytes());
    } else {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
}

fn varint(src: &[u8], pos: &mut usize, end: usize) -> Result<u32, Error> {
    let mut r = 0u32;
    for shift in [0, 7, 14, 21, 28] {
        let b = byte(src, pos, end)?;
        r |= ((b & 127) as u32) << shift;
        if b < 128 {
            return Ok(r);
        }
    }
    Err(BAD)
}

/// Mode 1 (triangles), header `0xe1`.
fn decode_triangles(out: &mut [u8], count: usize, stride: usize, src: &[u8]) -> Result<(), Error> {
    if !count.is_multiple_of(3) {
        return Err(err!("M17", "meshopt: TRIANGLES count is not a multiple of 3"));
    }
    match src.first() {
        Some(0xe1) => {}
        Some(h) if h & 0xf0 == 0xe0 => return Err(err!("M18", "meshopt: index codec version other than 1")),
        _ => return Err(err!("M19", "meshopt: bad TRIANGLES header")),
    }
    let tris = count / 3;
    if src.len() < 1 + tris + 16 {
        return Err(BAD);
    }
    let end = src.len() - 16;
    let aux = &src[end..];
    let mut pos = 1 + tris;
    // Unfilled entries are u32::MAX, as in meshoptimizer: a damaged stream
    // that reads one gets an index past any vertex buffer (0xffff in a
    // 16-bit output), not
    // a plausible vertex 0 that would draw a wrong triangle unnoticed.
    let mut edges = [[u32::MAX; 2]; 16];
    let mut verts = [u32::MAX; 16];
    let (mut eo, mut vo) = (0usize, 0usize);
    let (mut next, mut last) = (0u32, 0u32);
    let index = |v: u32, last: &mut u32| -> u32 {
        let d = if v & 1 != 0 { !(v >> 1) } else { v >> 1 };
        *last = last.wrapping_add(d);
        *last
    };
    for t in 0..tris {
        let code = src[1 + t];
        let (a, b, c);
        if code < 0xf0 {
            let e = edges[(eo.wrapping_sub(1 + (code >> 4) as usize)) & 15];
            (a, b) = (e[0], e[1]);
            let y = code & 15;
            c = match y {
                0 => {
                    next = next.wrapping_add(1);
                    next.wrapping_sub(1)
                }
                // FIFO index y counts back from the newest entry (0), as
                // the edge lookup above does; 0 itself means "next". The
                // 0xf0+ codes below store index + 1 instead.
                1..=12 => verts[(vo.wrapping_sub(1 + y as usize)) & 15],
                13 | 14 => {
                    last = if y == 13 { last.wrapping_sub(1) } else { last.wrapping_add(1) };
                    last
                }
                _ => index(varint(src, &mut pos, end)?, &mut last),
            };
            if !(1..=12).contains(&y) {
                verts[vo & 15] = c;
                vo += 1;
            }
            edges[eo & 15] = [c, b];
            edges[(eo + 1) & 15] = [a, c];
            eo += 2;
        } else {
            let (z, w);
            let explicit = code >= 0xfe;
            if explicit {
                let zw = byte(src, &mut pos, end)?;
                if zw == 0 {
                    next = 0;
                }
                (z, w) = (zw >> 4, zw & 15);
                a = if code == 0xfe {
                    next = next.wrapping_add(1);
                    next.wrapping_sub(1)
                } else {
                    index(varint(src, &mut pos, end)?, &mut last)
                };
            } else {
                let zw = aux[(code & 15) as usize];
                (z, w) = (zw >> 4, zw & 15);
                a = next;
                next = next.wrapping_add(1);
            }
            let pick = |f: u8, next: &mut u32, last: &mut u32, pos: &mut usize| -> Result<u32, Error> {
                Ok(match f {
                    0 => {
                        *next = next.wrapping_add(1);
                        next.wrapping_sub(1)
                    }
                    15 if explicit => index(varint(src, pos, end)?, last),
                    _ => verts[(vo.wrapping_sub(f as usize)) & 15],
                })
            };
            b = pick(z, &mut next, &mut last, &mut pos)?;
            c = pick(w, &mut next, &mut last, &mut pos)?;
            edges[eo & 15] = [b, a];
            edges[(eo + 1) & 15] = [c, b];
            edges[(eo + 2) & 15] = [a, c];
            eo += 3;
            verts[vo & 15] = a;
            vo += 1;
            for (f, v) in [(z, b), (w, c)] {
                if f == 0 || (explicit && f == 15) {
                    verts[vo & 15] = v;
                    vo += 1;
                }
            }
        }
        write_index(out, t * 3, stride, a);
        write_index(out, t * 3 + 1, stride, b);
        write_index(out, t * 3 + 2, stride, c);
    }
    if pos != end {
        return Err(BAD);
    }
    Ok(())
}

/// Mode 2 (indices), header `0xd1`.
fn decode_indices(out: &mut [u8], count: usize, stride: usize, src: &[u8]) -> Result<(), Error> {
    match src.first() {
        Some(0xd1) => {}
        Some(h) if h & 0xf0 == 0xd0 => return Err(err!("M20", "meshopt: index sequence codec version other than 1")),
        _ => return Err(err!("M21", "meshopt: bad INDICES header")),
    }
    if src.len() < 5 || count > src.len() - 5 {
        return Err(BAD);
    }
    let end = src.len() - 4;
    let mut pos = 1;
    let mut last = [0u32; 2];
    for i in 0..count {
        let v = varint(src, &mut pos, end)?;
        let d = if v & 2 != 0 { !(v >> 2) } else { v >> 2 };
        let l = &mut last[(v & 1) as usize];
        *l = l.wrapping_add(d);
        write_index(out, i, stride, *l);
    }
    if pos != end {
        return Err(BAD);
    }
    Ok(())
}

fn rd16(b: &[u8], i: usize) -> i16 {
    i16::from_le_bytes([b[i * 2], b[i * 2 + 1]])
}

fn wr16(b: &mut [u8], i: usize, v: i16) {
    b[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
}

/// Filter 1: octahedral unit vectors in 4 x i8 (stride 4) or 4 x i16 (stride 8).
fn filter_oct(data: &mut [u8], stride: usize) -> Result<(), Error> {
    let (max, wide) = match stride {
        4 => (127.0f32, false),
        8 => (32767.0f32, true),
        _ => return Err(err!("M22", "meshopt: OCTAHEDRAL needs byteStride 4 or 8")),
    };
    for e in data.chunks_exact_mut(stride) {
        let get = |e: &[u8], i: usize| if wide { rd16(e, i) as f32 } else { e[i] as i8 as f32 };
        let one = get(e, 2);
        let (mut x, mut y) = (get(e, 0), get(e, 1));
        let z = one - x.abs() - y.abs();
        let t = z.min(0.0);
        x -= t.copysign(x);
        y -= t.copysign(y);
        let len = (x * x + y * y + z * z).sqrt();
        let s = if len > 0.0 { max / len } else { 0.0 };
        let out = [(x * s).round(), (y * s).round(), (z * s).round()];
        for (i, v) in out.into_iter().enumerate() {
            if wide {
                wr16(e, i, v as i16);
            } else {
                e[i] = v as i8 as u8;
            }
        }
    }
    Ok(())
}

/// Filter 2: quaternions as three i16 components plus K-bit 1.0 and the index
/// of the dropped (largest) component.
fn filter_quat(data: &mut [u8], stride: usize) -> Result<(), Error> {
    if stride != 8 {
        return Err(err!("M23", "meshopt: QUATERNION needs byteStride 8"));
    }
    let range = std::f32::consts::FRAC_1_SQRT_2;
    for e in data.as_chunks_mut::<8>().0 {
        let last = rd16(e, 3);
        let one = (last | 3) as f32;
        let s = range / one;
        let (x, y, z) = (rd16(e, 0) as f32 * s, rd16(e, 1) as f32 * s, rd16(e, 2) as f32 * s);
        let w = (1.0 - x * x - y * y - z * z).max(0.0).sqrt();
        let m = (last & 3) as usize;
        for (k, v) in [(1, x), (2, y), (3, z), (0, w)] {
            wr16(e, (m + k) % 4, (v * 32767.0).round() as i16);
        }
    }
    Ok(())
}

/// Filter 3: exponential, each 32-bit word is `2^e * m` (e: top 8 bits signed,
/// m: low 24 bits signed), written back as an f32.
fn filter_exp(data: &mut [u8]) {
    for w in data.as_chunks_mut::<4>().0 {
        let v = i32::from_le_bytes([w[0], w[1], w[2], w[3]]);
        let e = v >> 24;
        let m = (v << 8) >> 8;
        // 2^e for e >= -126 is a normal f32; e = -127 and -128 halve the
        // result once or twice more (exact: m * 2^e is a multiple of 2^-128).
        let mut f = f32::from_bits(((e.max(-126) + 127) as u32) << 23) * m as f32;
        for _ in e..-126 {
            f *= 0.5;
        }
        w.copy_from_slice(&f.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_example_4bit_sentinel_group() {
        // Appendix A's worked example: 16 deltas in 4-bit sentinel coding.
        // One 1-byte element stride is not allowed (stride must be a
        // multiple of 4), so put the example in byte column 0 of a 4-byte
        // element and zeros in the other columns.
        let mut s = vec![0xa0];
        // Column 0: header (group 0 = mode 2), then the example bytes.
        s.push(0b10);
        s.extend_from_slice(&[0x17, 0x5f, 0xf0, 0xbc, 0x77, 0xa9, 0x21, 0x00, 0x34, 0xb5]);
        // Columns 1..3: all-zero groups.
        s.extend_from_slice(&[0, 0, 0]);
        // Tail: 32 bytes, baseline element (all zero) at the end.
        s.extend_from_slice(&[0u8; 32]);
        let mut out = vec![0u8; 16 * 4];
        decode_vertex_buffer(&mut out, 16, 4, &s).unwrap();
        let want: [i32; 16] = [-1, -4, -3, 26, -91, 0, -6, 6, -4, -4, 5, -5, 1, -1, 0, 0];
        let mut acc = 0i32;
        for (i, d) in want.iter().enumerate() {
            acc = (acc + d).rem_euclid(256);
            assert_eq!(out[i * 4] as i32, acc, "element {i}");
            assert_eq!(&out[i * 4 + 1..i * 4 + 4], &[0, 0, 0]);
        }
    }

    #[test]
    fn indices_mode_two_baselines() {
        // Deltas: +5 on baseline 0, +100 on baseline 1, +1 on baseline 0, -2 on baseline 1.
        let enc = |d: i32, b: u32| -> u32 { ((if d < 0 { !((d as u32) << 1) } else { (d as u32) << 1 }) << 1) | b };
        let mut s = vec![0xd1];
        for (d, b) in [(5, 0), (100, 1), (1, 0), (-2, 1)] {
            let mut v = enc(d, b);
            loop {
                if v < 128 {
                    s.push(v as u8);
                    break;
                }
                s.push((v as u8 & 127) | 128);
                v >>= 7;
            }
        }
        s.extend_from_slice(&[0; 4]);
        let mut out = vec![0u8; 16];
        decode_indices(&mut out, 4, 4, &s).unwrap();
        let got: Vec<u32> = out.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        assert_eq!(got, vec![5, 100, 6, 98]);
    }

    #[test]
    fn exponential_filter_is_two_to_the_e_times_m() {
        let mut d = Vec::new();
        for (e, m) in [(0i32, 3i32), (-2, 6), (4, -5), (-127, 1), (-128, -3), (-126, 1)] {
            d.extend_from_slice(&((e << 24) | (m & 0xff_ffff)).to_le_bytes());
        }
        filter_exp(&mut d);
        let f: Vec<f32> = d.chunks(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let tiny = f32::from_bits(1 << 23); // 2^-126
        assert_eq!(f, vec![3.0, 1.5, -80.0, tiny / 2.0, -3.0 * tiny / 4.0, tiny]);
    }

    #[test]
    fn unfilled_triangle_fifo_entries_are_out_of_range() {
        // Triangle 0 with code 0x10: an edge from a FIFO slot nothing wrote
        // yet, third vertex "next" (0). meshoptimizer starts its FIFOs at
        // 0xffffffff, so a damaged stream gives an impossible index, not a
        // plausible vertex 0.
        let mut s = vec![0xe1, 0x10];
        s.extend_from_slice(&[0u8; 16]);
        let mut out = vec![0u8; 12];
        decode_triangles(&mut out, 3, 4, &s).unwrap();
        let got: Vec<u32> = out.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        assert_eq!(got, vec![u32::MAX, u32::MAX, 0]);
    }

    #[test]
    fn octahedral_filter_decodes_axes() {
        // K = 8: one = 127. (0, 0) is +Z; (127, 0) is +X.
        let mut d = vec![0u8, 0, 127, 9, 127, 0, 127, 0];
        filter_oct(&mut d, 4).unwrap();
        assert_eq!(&d, &[0, 0, 127, 9, 127, 0, 0, 0]);
    }

    #[test]
    fn quaternion_filter_identity() {
        // Largest component w (index 3 in output) dropped: maxcomp = 3, so
        // input x,y,z map to outputs 0,1,2. K = 16: one = 32767 (bottom bits = 3).
        let mut d = Vec::new();
        for v in [0i16, 0, 0, 32767] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        filter_quat(&mut d, 8).unwrap();
        let q: Vec<i16> = d.chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(q, vec![0, 0, 0, 32767]);
    }
}
