//! A BC7 decoder, so the quality gate can score what a desktop GPU samples
//! from a tier's `bc7` texture set (S7-5). `ktx extract` decodes ASTC but
//! not BC7, and the gate's own Chrome may not sample BC7 at all, so the
//! level-0 blocks are decoded here, on the CPU, to the same 8-bit values the
//! GPU returns (BC7 decoding is exact: the format fixes every weight and
//! rounding step).
//!
//! Ported from bcdec by Sergii "iOrange" Kudlai
//! (<https://github.com/iOrange/bcdec>, `bcdec_bc7`), used under its MIT
//! licence (the file offers MIT or the Unlicense); the tables are the
//! format's own (Microsoft's "BC7 Format Mode Reference").

/// BC7's 2- and 3-subset partition tables: per partition, per texel (row
/// by row), the subset; 0x80 marks a subset's anchor texel, whose index has
/// one bit fewer.
const PARTITIONS: [[[u8; 16]; 64]; 2] = [
    [
        [128, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 129],
        [128, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 129],
        [128, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 129],
        [128, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 129],
        [128, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 129],
        [128, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 129],
        [128, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 129],
        [128, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 129],
        [128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 129],
        [128, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 129],
        [128, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 129],
        [128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 129],
        [128, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 129],
        [128, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 129],
        [128, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 129],
        [128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 129],
        [128, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 129],
        [128, 1, 129, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        [128, 0, 0, 0, 0, 0, 0, 0, 129, 0, 0, 0, 1, 1, 1, 0],
        [128, 1, 129, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0],
        [128, 0, 129, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        [128, 0, 0, 0, 1, 0, 0, 0, 129, 1, 0, 0, 1, 1, 1, 0],
        [128, 0, 0, 0, 0, 0, 0, 0, 129, 0, 0, 0, 1, 1, 0, 0],
        [128, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 129],
        [128, 0, 129, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0],
        [128, 0, 0, 0, 1, 0, 0, 0, 129, 0, 0, 0, 1, 1, 0, 0],
        [128, 1, 129, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0],
        [128, 0, 129, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0],
        [128, 0, 0, 1, 0, 1, 1, 1, 129, 1, 1, 0, 1, 0, 0, 0],
        [128, 0, 0, 0, 1, 1, 1, 1, 129, 1, 1, 1, 0, 0, 0, 0],
        [128, 1, 129, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0],
        [128, 0, 129, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0],
        [128, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 129],
        [128, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 129],
        [128, 1, 0, 1, 1, 0, 129, 0, 0, 1, 0, 1, 1, 0, 1, 0],
        [128, 0, 1, 1, 0, 0, 1, 1, 129, 1, 0, 0, 1, 1, 0, 0],
        [128, 0, 129, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0],
        [128, 1, 0, 1, 0, 1, 0, 1, 129, 0, 1, 0, 1, 0, 1, 0],
        [128, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 129],
        [128, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 129],
        [128, 1, 129, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0],
        [128, 0, 0, 1, 0, 0, 1, 1, 129, 1, 0, 0, 1, 0, 0, 0],
        [128, 0, 129, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0],
        [128, 0, 129, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0],
        [128, 1, 129, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0],
        [128, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 129],
        [128, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 129],
        [128, 0, 0, 0, 0, 1, 129, 0, 0, 1, 1, 0, 0, 0, 0, 0],
        [128, 1, 0, 0, 1, 1, 129, 0, 0, 1, 0, 0, 0, 0, 0, 0],
        [128, 0, 129, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0],
        [128, 0, 0, 0, 0, 0, 129, 0, 0, 1, 1, 1, 0, 0, 1, 0],
        [128, 0, 0, 0, 0, 1, 0, 0, 129, 1, 1, 0, 0, 1, 0, 0],
        [128, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 129],
        [128, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 129],
        [128, 1, 129, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0],
        [128, 0, 129, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0],
        [128, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 129],
        [128, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 129],
        [128, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 129],
        [128, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 129],
        [128, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 129],
        [128, 0, 129, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0],
        [128, 0, 129, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0],
        [128, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 129],
    ],
    [
        [128, 0, 1, 129, 0, 0, 1, 1, 0, 2, 2, 1, 2, 2, 2, 130],
        [128, 0, 0, 129, 0, 0, 1, 1, 130, 2, 1, 1, 2, 2, 2, 1],
        [128, 0, 0, 0, 2, 0, 0, 1, 130, 2, 1, 1, 2, 2, 1, 129],
        [128, 2, 2, 130, 0, 0, 2, 2, 0, 0, 1, 1, 0, 1, 1, 129],
        [128, 0, 0, 0, 0, 0, 0, 0, 129, 1, 2, 2, 1, 1, 2, 130],
        [128, 0, 1, 129, 0, 0, 1, 1, 0, 0, 2, 2, 0, 0, 2, 130],
        [128, 0, 2, 130, 0, 0, 2, 2, 1, 1, 1, 1, 1, 1, 1, 129],
        [128, 0, 1, 1, 0, 0, 1, 1, 130, 2, 1, 1, 2, 2, 1, 129],
        [128, 0, 0, 0, 0, 0, 0, 0, 129, 1, 1, 1, 2, 2, 2, 130],
        [128, 0, 0, 0, 1, 1, 1, 1, 129, 1, 1, 1, 2, 2, 2, 130],
        [128, 0, 0, 0, 1, 1, 129, 1, 2, 2, 2, 2, 2, 2, 2, 130],
        [128, 0, 1, 2, 0, 0, 129, 2, 0, 0, 1, 2, 0, 0, 1, 130],
        [128, 1, 1, 2, 0, 1, 129, 2, 0, 1, 1, 2, 0, 1, 1, 130],
        [128, 1, 2, 2, 0, 129, 2, 2, 0, 1, 2, 2, 0, 1, 2, 130],
        [128, 0, 1, 129, 0, 1, 1, 2, 1, 1, 2, 2, 1, 2, 2, 130],
        [128, 0, 1, 129, 2, 0, 0, 1, 130, 2, 0, 0, 2, 2, 2, 0],
        [128, 0, 0, 129, 0, 0, 1, 1, 0, 1, 1, 2, 1, 1, 2, 130],
        [128, 1, 1, 129, 0, 0, 1, 1, 130, 0, 0, 1, 2, 2, 0, 0],
        [128, 0, 0, 0, 1, 1, 2, 2, 129, 1, 2, 2, 1, 1, 2, 130],
        [128, 0, 2, 130, 0, 0, 2, 2, 0, 0, 2, 2, 1, 1, 1, 129],
        [128, 1, 1, 129, 0, 1, 1, 1, 0, 2, 2, 2, 0, 2, 2, 130],
        [128, 0, 0, 129, 0, 0, 0, 1, 130, 2, 2, 1, 2, 2, 2, 1],
        [128, 0, 0, 0, 0, 0, 129, 1, 0, 1, 2, 2, 0, 1, 2, 130],
        [128, 0, 0, 0, 1, 1, 0, 0, 130, 2, 129, 0, 2, 2, 1, 0],
        [128, 1, 2, 130, 0, 129, 2, 2, 0, 0, 1, 1, 0, 0, 0, 0],
        [128, 0, 1, 2, 0, 0, 1, 2, 129, 1, 2, 2, 2, 2, 2, 130],
        [128, 1, 1, 0, 1, 2, 130, 1, 129, 2, 2, 1, 0, 1, 1, 0],
        [128, 0, 0, 0, 0, 1, 129, 0, 1, 2, 130, 1, 1, 2, 2, 1],
        [128, 0, 2, 2, 1, 1, 0, 2, 129, 1, 0, 2, 0, 0, 2, 130],
        [128, 1, 1, 0, 0, 129, 1, 0, 2, 0, 0, 2, 2, 2, 2, 130],
        [128, 0, 1, 1, 0, 1, 2, 2, 0, 1, 130, 2, 0, 0, 1, 129],
        [128, 0, 0, 0, 2, 0, 0, 0, 130, 2, 1, 1, 2, 2, 2, 129],
        [128, 0, 0, 0, 0, 0, 0, 2, 129, 1, 2, 2, 1, 2, 2, 130],
        [128, 2, 2, 130, 0, 0, 2, 2, 0, 0, 1, 2, 0, 0, 1, 129],
        [128, 0, 1, 129, 0, 0, 1, 2, 0, 0, 2, 2, 0, 2, 2, 130],
        [128, 1, 2, 0, 0, 129, 2, 0, 0, 1, 130, 0, 0, 1, 2, 0],
        [128, 0, 0, 0, 1, 1, 129, 1, 2, 2, 130, 2, 0, 0, 0, 0],
        [128, 1, 2, 0, 1, 2, 0, 1, 130, 0, 129, 2, 0, 1, 2, 0],
        [128, 1, 2, 0, 2, 0, 1, 2, 129, 130, 0, 1, 0, 1, 2, 0],
        [128, 0, 1, 1, 2, 2, 0, 0, 1, 1, 130, 2, 0, 0, 1, 129],
        [128, 0, 1, 1, 1, 1, 130, 2, 2, 2, 0, 0, 0, 0, 1, 129],
        [128, 1, 0, 129, 0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 130],
        [128, 0, 0, 0, 0, 0, 0, 0, 130, 1, 2, 1, 2, 1, 2, 129],
        [128, 0, 2, 2, 1, 129, 2, 2, 0, 0, 2, 2, 1, 1, 2, 130],
        [128, 0, 2, 130, 0, 0, 1, 1, 0, 0, 2, 2, 0, 0, 1, 129],
        [128, 2, 2, 0, 1, 2, 130, 1, 0, 2, 2, 0, 1, 2, 2, 129],
        [128, 1, 0, 1, 2, 2, 130, 2, 2, 2, 2, 2, 0, 1, 0, 129],
        [128, 0, 0, 0, 2, 1, 2, 1, 130, 1, 2, 1, 2, 1, 2, 129],
        [128, 1, 0, 129, 0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 130],
        [128, 2, 2, 130, 0, 1, 1, 1, 0, 2, 2, 2, 0, 1, 1, 129],
        [128, 0, 0, 2, 1, 129, 1, 2, 0, 0, 0, 2, 1, 1, 1, 130],
        [128, 0, 0, 0, 2, 129, 1, 2, 2, 1, 1, 2, 2, 1, 1, 130],
        [128, 2, 2, 2, 0, 129, 1, 1, 0, 1, 1, 1, 0, 2, 2, 130],
        [128, 0, 0, 2, 1, 1, 1, 2, 129, 1, 1, 2, 0, 0, 0, 130],
        [128, 1, 1, 0, 0, 129, 1, 0, 0, 1, 1, 0, 2, 2, 2, 130],
        [128, 0, 0, 0, 0, 0, 0, 0, 2, 1, 129, 2, 2, 1, 1, 130],
        [128, 1, 1, 0, 0, 129, 1, 0, 2, 2, 2, 2, 2, 2, 2, 130],
        [128, 0, 2, 2, 0, 0, 1, 1, 0, 0, 129, 1, 0, 0, 2, 130],
        [128, 0, 2, 2, 1, 1, 2, 2, 129, 1, 2, 2, 0, 0, 2, 130],
        [128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 129, 1, 130],
        [128, 0, 0, 130, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 129],
        [128, 2, 2, 2, 1, 2, 2, 2, 0, 2, 2, 2, 129, 2, 2, 130],
        [128, 1, 0, 129, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 130],
        [128, 1, 1, 129, 2, 0, 1, 1, 130, 2, 0, 1, 2, 2, 2, 0],
    ],
];

const WEIGHTS2: [u32; 4] = [0, 21, 43, 64];
const WEIGHTS3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const WEIGHTS4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

/// Colour and alpha endpoint bits per mode (without the p-bit).
const COLOUR_BITS: [u32; 8] = [4, 6, 5, 7, 5, 7, 7, 5];
const ALPHA_BITS: [u32; 8] = [0, 0, 0, 0, 6, 8, 7, 5];
/// Modes with a p-bit per endpoint (0, 3, 6, 7) or one per subset (1).
const HAS_PBITS: u8 = 0b1100_1011;

/// A 128-bit little-endian bit reader.
struct Bits(u128);

impl Bits {
    fn take(&mut self, n: u32) -> u32 {
        let v = (self.0 & ((1u128 << n) - 1)) as u32;
        self.0 >>= n;
        v
    }
}

fn weights(bits: u32) -> &'static [u32] {
    match bits {
        2 => &WEIGHTS2,
        3 => &WEIGHTS3,
        _ => &WEIGHTS4,
    }
}

fn lerp(a: u32, b: u32, w: &[u32], i: u32) -> u8 {
    let w = w.get(i as usize).copied().unwrap_or(0);
    ((a * (64 - w) + b * w + 32) >> 6) as u8
}

/// One 16-byte BC7 block as 16 RGBA texels, row by row. A block with no
/// valid mode decodes to transparent black, as the format says.
pub fn decode_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let mut out = [[0u8; 4]; 16];
    let mut b = Bits(u128::from_le_bytes(*block));
    let mut mode = 0usize;
    while mode < 8 && b.take(1) == 0 {
        mode += 1;
    }
    if mode >= 8 {
        return out;
    }
    let subsets = match mode {
        0 | 2 => 3,
        1 | 3 | 7 => 2,
        _ => 1,
    };
    let partition = match mode {
        0 => b.take(4),
        1 | 2 | 3 | 7 => b.take(6),
        _ => 0,
    } as usize;
    let (rotation, isb) = match mode {
        4 => (b.take(2), b.take(1)),
        5 => (b.take(2), 0),
        _ => (0, 0),
    };
    let n = subsets * 2;
    let mut ep = [[0u32; 4]; 6];
    for c in 0..3 {
        for e in ep.iter_mut().take(n) {
            e[c] = b.take(COLOUR_BITS[mode]);
        }
    }
    if ALPHA_BITS[mode] > 0 {
        for e in ep.iter_mut().take(n) {
            e[3] = b.take(ALPHA_BITS[mode]);
        }
    }
    let pbit = u32::from((HAS_PBITS >> mode) & 1);
    if pbit == 1 {
        for e in ep.iter_mut().take(n) {
            for c in e.iter_mut() {
                *c <<= 1;
            }
        }
        if mode == 1 {
            let (p0, p1) = (b.take(1), b.take(1));
            for (k, e) in ep.iter_mut().take(4).enumerate() {
                let p = if k < 2 { p0 } else { p1 };
                for c in e.iter_mut().take(3) {
                    *c |= p;
                }
            }
        } else {
            for e in ep.iter_mut().take(n) {
                let p = b.take(1);
                for c in e.iter_mut() {
                    *c |= p;
                }
            }
        }
    }
    for e in ep.iter_mut().take(n) {
        let j = COLOUR_BITS[mode] + pbit;
        for c in e.iter_mut().take(3) {
            *c <<= 8 - j;
            *c |= *c >> j;
        }
        if ALPHA_BITS[mode] > 0 {
            let j = ALPHA_BITS[mode] + pbit;
            e[3] <<= 8 - j;
            e[3] |= e[3] >> j;
        } else {
            e[3] = 255;
        }
    }
    let ib = match mode {
        0 | 1 => 3,
        6 => 4,
        _ => 2,
    };
    let ib2 = match mode {
        4 => 3,
        5 => 2,
        _ => 0,
    };
    let subset_of = |t: usize| -> u8 {
        if subsets == 1 {
            if t == 0 { 128 } else { 0 }
        } else {
            PARTITIONS[subsets - 2][partition][t]
        }
    };
    let mut idx = [0u32; 16];
    for (t, i) in idx.iter_mut().enumerate() {
        let bits = if subset_of(t) & 0x80 != 0 { ib - 1 } else { ib };
        *i = b.take(bits);
    }
    let (w1, w2) = (weights(ib), weights(ib2));
    for (t, px) in out.iter_mut().enumerate() {
        let s = usize::from(subset_of(t) & 0x03);
        let (a, z) = (ep[s * 2], ep[s * 2 + 1]);
        let i = idx[t];
        let mut c = if ib2 == 0 {
            [lerp(a[0], z[0], w1, i), lerp(a[1], z[1], w1, i), lerp(a[2], z[2], w1, i), lerp(a[3], z[3], w1, i)]
        } else {
            let i2 = b.take(if t == 0 { ib2 - 1 } else { ib2 });
            if isb == 0 {
                [lerp(a[0], z[0], w1, i), lerp(a[1], z[1], w1, i), lerp(a[2], z[2], w1, i), lerp(a[3], z[3], w2, i2)]
            } else {
                [lerp(a[0], z[0], w2, i2), lerp(a[1], z[1], w2, i2), lerp(a[2], z[2], w2, i2), lerp(a[3], z[3], w1, i)]
            }
        };
        match rotation {
            1 => c.swap(0, 3),
            2 => c.swap(1, 3),
            3 => c.swap(2, 3),
            _ => {}
        }
        *px = c;
    }
    out
}

/// A BC7 KTX2 file's level 0 as (width, height, RGBA rows). Refuses any
/// other format, supercompression, or a level outside the file.
pub fn decode_ktx2_level0(bytes: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let vk = crate::texture::ktx2_vk_format(bytes).ok_or("not a KTX2 file")?;
    if !matches!(vk, 145 | 146) {
        return Err(format!("vkFormat {vk} is not BC7"));
    }
    let u32_at = |o: usize| bytes.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or("truncated KTX2 header");
    let u64_at = |o: usize| bytes.get(o..o + 8).map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])).ok_or("truncated KTX2 level index");
    if u32_at(44)? != 0 {
        return Err("supercompressed KTX2".into());
    }
    let (w, h) = (u32_at(20)? as usize, u32_at(24)? as usize);
    let (off, len) = (u64_at(80)? as usize, u64_at(88)? as usize);
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    if w == 0 || h == 0 || len < bw * bh * 16 {
        return Err(format!("level 0 of {len} bytes for {w}x{h}"));
    }
    let data = bytes.get(off..off + len).ok_or("level 0 lies outside the file")?;
    let mut px = vec![0u8; w * h * 4];
    for (k, blk) in data.as_chunks::<16>().0.iter().take(bw * bh).enumerate() {
        let (bx, by) = ((k % bw) * 4, (k / bw) * 4);
        for (t, c) in decode_block(blk).iter().enumerate() {
            let (x, y) = (bx + t % 4, by + t / 4);
            if x < w && y < h {
                px[(y * w + x) * 4..][..4].copy_from_slice(c);
            }
        }
    }
    Ok((w, h, px))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_6_solid_block() {
        // Mode 6 (bit 6 set), endpoints 7 bits + p-bit; all colour bits set,
        // p-bits 1: both endpoints are 255, so every texel is opaque white.
        let mut v: u128 = 1 << 6;
        let mut at = 7u32;
        for _ in 0..8 {
            v |= 0x7f << at;
            at += 7;
        }
        v |= 0b11 << at;
        let px = decode_block(&v.to_le_bytes());
        assert!(px.iter().all(|p| *p == [255, 255, 255, 255]), "{px:?}");
    }

    #[test]
    fn no_mode_is_transparent_black() {
        assert_eq!(decode_block(&[0; 16]), [[0; 4]; 16]);
    }

    #[test]
    fn refuses_other_formats() {
        assert!(decode_ktx2_level0(b"not a ktx2").is_err());
    }
}
