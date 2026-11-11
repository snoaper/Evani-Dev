//! The 32-byte material block (BATCH1_BUILD_PLAN section 2.2).
//!
//! Every catalogue row encodes to one fixed block. It travels inside the models
//! that use it as 64 hex characters (`extras.evani_recipe`), so a game downloads
//! about 32 bytes per material and never the names or notes. On the GPU the same
//! bytes are two `vec4<u32>`.
//!
//! | Bytes | Field |
//! |---|---|
//! | 0 | recipe code (1-29, 0 = no recipe) |
//! | 1 | variant (the recipe's pattern kind) |
//! | 2 | extras mask |
//! | 3 | condition mask (bit 0 wet, 1 mud, 2 dust, 3 snow, 4 wear, 5 oxide, 6 growth, 7 blood) |
//! | 4-6, 7-9, 10-12 | colours A, B, C (sRGB8) |
//! | 13-22 | ten recipe numbers n0-n9 (u8 unorm over each number's range) |
//! | 23 | seed (low byte of FNV-1a 32 of the material id) |
//! | 24-31 | default amount of each of the 8 condition layers (u8 unorm) |
//!
//! No dependency, no allocation except the hex string, fully deterministic.

/// Size of one block in bytes.
pub const BLOCK_BYTES: usize = 32;
/// Hex characters of one block as stored in a model.
pub const BLOCK_HEX_LEN: usize = BLOCK_BYTES * 2;
/// Recipe numbers per block.
pub const NUMBER_COUNT: usize = 10;
/// Colours per block.
pub const COLOUR_COUNT: usize = 3;
/// Condition layers; also the bits of the condition mask.
pub const CONDITION_COUNT: usize = 8;

/// Condition layer names, in bit order.
pub const CONDITION_NAMES: [&str; CONDITION_COUNT] = ["wet", "mud", "dust", "snow", "wear", "oxide", "growth", "blood"];

/// The decoded fields of a block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fields {
    /// Recipe code, 1-29 (0 = no recipe).
    pub recipe: u8,
    /// The recipe's variant (pattern kind).
    pub variant: u8,
    /// Bits of the recipe's extras this material uses.
    pub extras: u8,
    /// Condition layers this material accepts (one bit per layer).
    pub cond_mask: u8,
    /// Colours A, B and C as sRGB8.
    pub colours: [[u8; 3]; COLOUR_COUNT],
    /// Recipe numbers n0-n9 as u8 unorm.
    pub numbers: [u8; NUMBER_COUNT],
    /// Fixed pattern offset: low byte of FNV-1a 32 of the id.
    pub seed: u8,
    /// Default amount of each condition layer, u8 unorm.
    pub cond_amounts: [u8; CONDITION_COUNT],
}

/// One 32-byte material block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block(pub [u8; BLOCK_BYTES]);

/// Why a hex string is not a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HexError {
    /// Not 64 characters.
    Length(usize),
    /// A character that is not 0-9 or a-f (A-F is accepted too).
    BadChar,
}

impl core::fmt::Display for HexError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HexError::Length(n) => write!(f, "a material block is {BLOCK_HEX_LEN} hex characters, got {n}"),
            HexError::BadChar => write!(f, "a material block holds only hex digits"),
        }
    }
}

impl core::error::Error for HexError {}

impl Block {
    /// The block with every byte zero: no recipe.
    pub const EMPTY: Block = Block([0; BLOCK_BYTES]);

    /// Pack decoded fields into a block.
    pub fn pack(f: &Fields) -> Block {
        let mut b = [0u8; BLOCK_BYTES];
        b[0] = f.recipe;
        b[1] = f.variant;
        b[2] = f.extras;
        b[3] = f.cond_mask;
        for (i, c) in f.colours.iter().enumerate() {
            b[4 + i * 3..7 + i * 3].copy_from_slice(c);
        }
        b[13..23].copy_from_slice(&f.numbers);
        b[23] = f.seed;
        b[24..32].copy_from_slice(&f.cond_amounts);
        Block(b)
    }

    /// Decode a block into its fields.
    pub fn unpack(&self) -> Fields {
        let b = &self.0;
        let mut colours = [[0u8; 3]; COLOUR_COUNT];
        for (i, c) in colours.iter_mut().enumerate() {
            c.copy_from_slice(&b[4 + i * 3..7 + i * 3]);
        }
        let mut numbers = [0u8; NUMBER_COUNT];
        numbers.copy_from_slice(&b[13..23]);
        let mut cond_amounts = [0u8; CONDITION_COUNT];
        cond_amounts.copy_from_slice(&b[24..32]);
        Fields { recipe: b[0], variant: b[1], extras: b[2], cond_mask: b[3], colours, numbers, seed: b[23], cond_amounts }
    }

    /// Lower-case hex, 64 characters (what `extras.evani_recipe` holds).
    pub fn to_hex(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(BLOCK_HEX_LEN);
        for &byte in &self.0 {
            s.push(DIGITS[(byte >> 4) as usize] as char);
            s.push(DIGITS[(byte & 15) as usize] as char);
        }
        s
    }

    /// Parse the hex form back into a block.
    pub fn from_hex(s: &str) -> Result<Block, HexError> {
        let bytes = s.as_bytes();
        if bytes.len() != BLOCK_HEX_LEN {
            return Err(HexError::Length(bytes.len()));
        }
        let mut out = [0u8; BLOCK_BYTES];
        for (i, byte) in out.iter_mut().enumerate() {
            let hi = hex_digit(bytes[i * 2]).ok_or(HexError::BadChar)?;
            let lo = hex_digit(bytes[i * 2 + 1]).ok_or(HexError::BadChar)?;
            *byte = (hi << 4) | lo;
        }
        Ok(Block(out))
    }

    /// The block as 8 little-endian u32 words, the order the GPU reads them
    /// (`vec4<u32>` x 2, then `unpack4x8unorm` per word).
    pub fn to_words(&self) -> [u32; 8] {
        let mut w = [0u32; 8];
        for (i, word) in w.iter_mut().enumerate() {
            let k = i * 4;
            *word = u32::from_le_bytes([self.0[k], self.0[k + 1], self.0[k + 2], self.0[k + 3]]);
        }
        w
    }
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// FNV-1a, 32 bit.
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// The seed byte of a material id: the low byte of its FNV-1a 32 hash.
pub fn seed_byte(id: &str) -> u8 {
    (fnv1a32(id.as_bytes()) & 0xff) as u8
}

/// Quantise `v` over `[min, max]` to u8 unorm. Out-of-range and NaN clamp.
pub fn quantise(v: f32, min: f32, max: f32) -> u8 {
    if max <= min || v.is_nan() {
        return 0;
    }
    let t = ((v - min) / (max - min)).clamp(0.0, 1.0);
    (t * 255.0 + 0.5) as u8
}

/// The value a u8 stands for over `[min, max]`.
pub fn dequantise(q: u8, min: f32, max: f32) -> f32 {
    min + f32::from(q) / 255.0 * (max - min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_known_vectors() {
        assert_eq!(fnv1a32(b""), 0x811c_9dc5);
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c);
        assert_eq!(fnv1a32(b"foobar"), 0xbf9c_f968);
        assert_eq!(seed_byte("a"), 0x2c);
    }

    #[test]
    fn pack_unpack_round_trip() {
        let f = Fields {
            recipe: 12,
            variant: 3,
            extras: 0b10,
            cond_mask: 0b1001_1011,
            colours: [[1, 2, 3], [4, 5, 6], [7, 8, 9]],
            numbers: [10, 11, 12, 13, 14, 15, 16, 17, 18, 19],
            seed: 0xab,
            cond_amounts: [20, 21, 22, 23, 24, 25, 26, 27],
        };
        let b = Block::pack(&f);
        assert_eq!(b.0.len(), 32);
        assert_eq!(b.0[0], 12);
        assert_eq!(b.0[4], 1);
        assert_eq!(b.0[12], 9);
        assert_eq!(b.0[13], 10);
        assert_eq!(b.0[22], 19);
        assert_eq!(b.0[23], 0xab);
        assert_eq!(b.0[24], 20);
        assert_eq!(b.0[31], 27);
        assert_eq!(b.unpack(), f);
    }

    #[test]
    fn hex_round_trip_and_errors() {
        let b = Block::pack(&Fields { recipe: 29, seed: 0xff, ..Fields::default() });
        let h = b.to_hex();
        assert_eq!(h.len(), 64);
        assert_eq!(Block::from_hex(&h), Ok(b));
        assert_eq!(Block::from_hex(&h.to_uppercase()), Ok(b));
        assert_eq!(Block::from_hex("00"), Err(HexError::Length(2)));
        assert_eq!(Block::from_hex(&"g".repeat(64)), Err(HexError::BadChar));
    }

    #[test]
    fn words_are_little_endian() {
        let mut b = Block::EMPTY;
        b.0[0] = 1;
        b.0[3] = 0x80;
        assert_eq!(b.to_words()[0], 0x8000_0001);
    }

    #[test]
    fn quantise_clamps_and_round_trips() {
        assert_eq!(quantise(0.0, 0.0, 1.0), 0);
        assert_eq!(quantise(1.0, 0.0, 1.0), 255);
        assert_eq!(quantise(-5.0, 0.0, 1.0), 0);
        assert_eq!(quantise(9.0, 0.0, 1.0), 255);
        assert_eq!(quantise(f32::NAN, 0.0, 1.0), 0);
        assert_eq!(quantise(0.5, 1.0, 1.0), 0);
        for q in 0..=255u8 {
            assert_eq!(quantise(dequantise(q, 3.0, 40.0), 3.0, 40.0), q);
        }
    }
}
