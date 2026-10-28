//! Model textures on the CPU side (feature `models`): which per-device set to
//! fetch, the KTX2 container header, and the tier's texture memory budget.
//! Pure functions, so they are tested natively; the GPU side
//! (`gpu::models`) only uploads what they decide.

/// The texture formats a model texture can arrive in, in the order the
/// loader prefers them (KTX2Loader's order: ASTC 4x4, BC7, BC1/BC3, ETC2,
/// then plain RGBA).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexFormat {
    Astc4x4,
    Bc7,
    /// BC1 (`opaque`) or BC3, delivered in a `bc` set.
    Bc13,
    Etc2,
    /// The file's own core image (PNG or JPEG), decoded by the browser.
    Rgba,
}

/// The block-compression families the device was created with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceTex {
    pub astc: bool,
    pub bc: bool,
    pub etc2: bool,
}

/// Pick the set to fetch: the first of ASTC, BC7, BC1/BC3 and ETC2 that the
/// file lists and the device can sample (ASTC whenever both are reported),
/// else [`TexFormat::Rgba`]. `sets` are the format names of
/// `extras.evani.textures[].sets` (`astc`, `bc7`, `bc`, `etc2`, `rgba`).
pub fn choose_format<'a>(sets: impl Iterator<Item = &'a str> + Clone, dev: DeviceTex) -> TexFormat {
    let has = |n: &str| sets.clone().any(|s| s == n);
    if dev.astc && has("astc") {
        TexFormat::Astc4x4
    } else if dev.bc && has("bc7") {
        TexFormat::Bc7
    } else if dev.bc && has("bc") {
        TexFormat::Bc13
    } else if dev.etc2 && has("etc2") {
        TexFormat::Etc2
    } else {
        TexFormat::Rgba
    }
}

/// The set name a [`TexFormat`] is listed under.
pub fn set_name(f: TexFormat) -> &'static str {
    match f {
        TexFormat::Astc4x4 => "astc",
        TexFormat::Bc7 => "bc7",
        TexFormat::Bc13 => "bc",
        TexFormat::Etc2 => "etc2",
        TexFormat::Rgba => "rgba",
    }
}

/// What a KTX2 file's header says (KTX 2.0 specification, section 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ktx2 {
    /// Vulkan format number (`VK_FORMAT_*`).
    pub vk_format: u32,
    pub width: u32,
    pub height: u32,
    /// `(byte offset, byte length)` of each mip level, level 0 (largest)
    /// first; every range lies inside the file.
    pub levels: Vec<(usize, usize)>,
}

/// The 12-byte KTX2 identifier.
pub const KTX2_MAGIC: [u8; 12] = [0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A];

/// The texel block of a format: `(block width and height, bytes per block)`.
/// `None` for a format the engine does not upload.
pub fn block_of(vk_format: u32) -> Option<(u32, u32)> {
    match vk_format {
        // R8G8B8A8_UNORM, R8G8B8A8_SRGB
        37 | 43 => Some((1, 4)),
        // BC1 RGB/RGBA (unorm, srgb)
        131..=134 => Some((4, 8)),
        // BC3, BC7 (unorm, srgb)
        137 | 138 | 145 | 146 => Some((4, 16)),
        // ETC2 RGB8 and RGB8A1 (unorm, srgb)
        147..=150 => Some((4, 8)),
        // ETC2 RGBA8 (unorm, srgb)
        151 | 152 => Some((4, 16)),
        // ASTC 4x4 (unorm, srgb)
        157 | 158 => Some((4, 16)),
        _ => None,
    }
}

/// The family a Vulkan format belongs to, for checking a set holds what its
/// name says.
pub fn family_of(vk_format: u32) -> Option<TexFormat> {
    match vk_format {
        37 | 43 => Some(TexFormat::Rgba),
        131..=134 | 137 | 138 => Some(TexFormat::Bc13),
        145 | 146 => Some(TexFormat::Bc7),
        147..=152 => Some(TexFormat::Etc2),
        157 | 158 => Some(TexFormat::Astc4x4),
        _ => None,
    }
}

/// Whether a format stores sRGB-encoded colour.
pub fn is_srgb(vk_format: u32) -> bool {
    matches!(vk_format, 43 | 132 | 134 | 138 | 146 | 148 | 150 | 152 | 158)
}

/// Bytes of one mip level of `w x h` texels in a format with `block`.
pub fn level_bytes(w: u32, h: u32, block: (u32, u32)) -> u64 {
    let (b, bytes) = block;
    u64::from(w.div_ceil(b).max(1)) * u64::from(h.div_ceil(b).max(1)) * u64::from(bytes)
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from(u32_at(b, at)?) | (u64::from(u32_at(b, at + 4)?) << 32))
}

/// Read a KTX2 header and level index. Refused (an error, never a panic):
/// a wrong identifier, supercompression (the server compresses the bytes on
/// the wire, so files carry none), a 3D, array or cube texture, a format the
/// engine does not upload, a level whose bytes run past the file or whose
/// length is not what its size and format need.
pub fn parse_ktx2(b: &[u8]) -> Result<Ktx2, &'static str> {
    if b.get(..12) != Some(&KTX2_MAGIC[..]) {
        return Err("ktx2: not a KTX2 file");
    }
    let field = |at: usize| u32_at(b, at).ok_or("ktx2: truncated header");
    let vk_format = field(12)?;
    let (width, height, depth, layers, faces) = (field(20)?, field(24)?, field(28)?, field(32)?, field(36)?);
    let level_count = field(40)?.max(1);
    if field(44)? != 0 {
        return Err("ktx2: supercompressed (the engine reads plain levels)");
    }
    if width == 0 || height == 0 || depth > 1 || layers > 1 || faces != 1 {
        return Err("ktx2: only plain 2D textures are read");
    }
    let block = block_of(vk_format).ok_or("ktx2: a format the engine does not upload")?;
    if level_count > 16 || (width >> (level_count - 1)) == 0 && (height >> (level_count - 1)) == 0 {
        return Err("ktx2: more levels than the size allows");
    }
    let mut levels = Vec::with_capacity(level_count as usize);
    for i in 0..level_count {
        let at = 80 + 24 * i as usize;
        let off = u64_at(b, at).ok_or("ktx2: truncated level index")?;
        let len = u64_at(b, at + 8).ok_or("ktx2: truncated level index")?;
        let end = off.checked_add(len).ok_or("ktx2: bad level range")?;
        if end > b.len() as u64 {
            return Err("ktx2: a level runs past the end of the file");
        }
        if len != level_bytes((width >> i).max(1), (height >> i).max(1), block) {
            return Err("ktx2: a level's length does not match its size");
        }
        levels.push((off as usize, len as usize));
    }
    Ok(Ktx2 { vk_format, width, height, levels })
}

/// The mip chain length of a `w x h` texture (down to 1 x 1).
pub fn full_levels(w: u32, h: u32) -> u32 {
    32 - w.max(h).max(1).leading_zeros()
}

/// One texture as the budget sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TexSize {
    pub width: u32,
    pub height: u32,
    pub levels: u32,
    /// Texel block of its format (see [`block_of`]).
    pub block: (u32, u32),
}

impl TexSize {
    /// GPU bytes with the top `skip` levels dropped.
    pub fn bytes(&self, skip: u32) -> u64 {
        (skip..self.levels.max(1)).map(|i| level_bytes((self.width >> i).max(1), (self.height >> i).max(1), self.block)).sum()
    }
}

/// Hold `textures` to `budget` bytes: while over, the texture whose
/// resident top level is largest drops that level (its next level becomes
/// the top), so the biggest textures lose detail first and small ones keep
/// theirs. A texture keeps at least one level, so the result can still be
/// over budget. Returns how many top levels each texture drops.
pub fn drop_top_mips(textures: &[TexSize], budget: u64) -> Vec<u32> {
    let mut skip = vec![0u32; textures.len()];
    let mut total: u64 = textures.iter().map(|t| t.bytes(0)).sum();
    while total > budget {
        let top = |i: usize| {
            let t = &textures[i];
            let s = skip[i];
            if s + 1 >= t.levels.max(1) { 0 } else { level_bytes((t.width >> s).max(1), (t.height >> s).max(1), t.block) }
        };
        let Some(i) = (0..textures.len()).filter(|&i| top(i) > 0).max_by_key(|&i| (top(i), core::cmp::Reverse(i))) else {
            break;
        };
        total -= top(i);
        skip[i] += 1;
    }
    skip
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A KTX2 file with the given format, size and level byte lengths
    /// (levels laid out back to back after the index).
    fn ktx2(vk: u32, w: u32, h: u32, lens: &[u64], supercompression: u32) -> Vec<u8> {
        let mut b = KTX2_MAGIC.to_vec();
        for v in [vk, 1, w, h, 0, 0, 1, lens.len() as u32, supercompression] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.resize(80, 0);
        let mut off = 80 + 24 * lens.len() as u64;
        for &l in lens {
            for v in [off, l, l] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            off += l;
        }
        b.resize(off as usize, 7);
        b
    }

    #[test]
    fn ktx2_header_reads_levels_and_format() {
        // BC7 sRGB 8x8 with 4 levels: 4 blocks, then 1, 1, 1 block of 16 B.
        let f = ktx2(146, 8, 8, &[64, 16, 16, 16], 0);
        let k = parse_ktx2(&f).unwrap();
        assert_eq!((k.vk_format, k.width, k.height), (146, 8, 8));
        assert_eq!(k.levels.len(), 4);
        assert_eq!(k.levels[0], (80 + 96, 64));
        assert_eq!(k.levels[1], (80 + 96 + 64, 16));
        assert!(is_srgb(k.vk_format) && family_of(k.vk_format) == Some(TexFormat::Bc7));
        // ASTC 4x4 unorm 16x8, 2 levels (8 then 2 blocks).
        let k = parse_ktx2(&ktx2(157, 16, 8, &[128, 32], 0)).unwrap();
        assert_eq!(k.levels.iter().map(|l| l.1).collect::<Vec<_>>(), [128, 32]);
        assert!(!is_srgb(157) && family_of(157) == Some(TexFormat::Astc4x4));
        // RGBA8, level count 0 means one level.
        let k = parse_ktx2(&ktx2(43, 2, 2, &[16], 0)).unwrap();
        assert_eq!(k.levels, [(80 + 24, 16)]);
        // Refused: supercompression, a bad identifier, a wrong level length,
        // a truncated file, an unknown format, a 2-level 1x1 chain.
        assert!(parse_ktx2(&ktx2(146, 8, 8, &[64], 2)).is_err());
        let mut bad = ktx2(146, 8, 8, &[64], 0);
        bad[1] = b'X';
        assert!(parse_ktx2(&bad).is_err());
        assert!(parse_ktx2(&ktx2(146, 8, 8, &[60], 0)).is_err());
        let f = ktx2(146, 8, 8, &[64], 0);
        assert!(parse_ktx2(&f[..f.len() - 1]).is_err());
        assert!(parse_ktx2(&f[..50]).is_err());
        assert!(parse_ktx2(&ktx2(999, 8, 8, &[64], 0)).is_err());
        assert!(parse_ktx2(&ktx2(43, 1, 1, &[4, 4], 0)).is_err());
        assert!(parse_ktx2(&[]).is_err());
    }

    #[test]
    fn format_choice_prefers_astc_over_bc() {
        let all = ["rgba", "etc2", "bc7", "astc"];
        let dev = |astc, bc, etc2| DeviceTex { astc, bc, etc2 };
        // A device reporting both ASTC and BC gets ASTC.
        assert_eq!(choose_format(all.iter().copied(), dev(true, true, true)), TexFormat::Astc4x4);
        assert_eq!(choose_format(all.iter().copied(), dev(false, true, true)), TexFormat::Bc7);
        assert_eq!(choose_format(all.iter().copied(), dev(false, false, true)), TexFormat::Etc2);
        assert_eq!(choose_format(all.iter().copied(), dev(false, false, false)), TexFormat::Rgba);
        // Only what the file lists: no astc set means BC7 on an ASTC + BC device.
        assert_eq!(choose_format(["bc7", "etc2"].iter().copied(), dev(true, true, false)), TexFormat::Bc7);
        // BC1/BC3 after BC7, before ETC2.
        assert_eq!(choose_format(["bc", "etc2"].iter().copied(), dev(false, true, true)), TexFormat::Bc13);
        // No sets at all: the core image.
        assert_eq!(choose_format([].iter().copied(), dev(true, true, true)), TexFormat::Rgba);
        assert_eq!(set_name(TexFormat::Astc4x4), "astc");
    }

    #[test]
    fn texture_budget_drops_top_mips_first() {
        let rgba = |w: u32| TexSize { width: w, height: w, levels: full_levels(w, w), block: (1, 4) };
        let big = rgba(1024);
        let small = rgba(256);
        assert_eq!(full_levels(1024, 1024), 11);
        assert_eq!(full_levels(1, 1), 1);
        assert_eq!(full_levels(300, 20), 9);
        let total = big.bytes(0) + small.bytes(0);
        // Within budget: nothing drops.
        assert_eq!(drop_top_mips(&[big, small], total), [0, 0]);
        // Just over: the big texture loses its top level, the small one keeps all.
        assert_eq!(drop_top_mips(&[big, small], total - 1), [1, 0]);
        // Tighter: the big one keeps dropping until its top is no larger than
        // the small one's, then they take turns (largest top first).
        let budget = big.bytes(2) + small.bytes(0);
        assert_eq!(drop_top_mips(&[small, big], budget), [0, 2]);
        let budget = big.bytes(3) + small.bytes(1);
        assert_eq!(drop_top_mips(&[big, small], budget), [3, 1]);
        // Every texture keeps one level even if that is still over budget.
        let one = TexSize { width: 4, height: 4, levels: 1, block: (4, 16) };
        assert_eq!(drop_top_mips(&[one, big], 0), [0, 10]);
        // Block formats count whole blocks.
        assert_eq!(level_bytes(1, 1, (4, 16)), 16);
        assert_eq!(level_bytes(5, 4, (4, 8)), 16);
        assert_eq!(TexSize { width: 8, height: 8, levels: 4, block: (4, 16) }.bytes(0), 64 + 16 + 16 + 16);
    }
}
