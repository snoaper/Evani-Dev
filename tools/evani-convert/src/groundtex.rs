//! `cargo xtask convert-textures <layers.json> --out <game assets>`: tiling
//! PBR layers for evani-render's `kit-textures` (TopGun W3,
//! docs/GROUND_TEXTURES.md). Our own code end to end except the pinned
//! `ktx` tool for the ASTC and BC7 blocks.
//!
//! Per layer:
//! 1. level 0 of each map at the largest size any tier asks for: decoded
//!    photo maps (albedo, OpenGL normal, roughness, AO, height; sharp from
//!    the pinned npm tree) or one of our own procedural recipes
//!    ([`procedural`]: `rock`, `field`, `paving`), all tiling;
//! 2. the mip chain, built here: a Kaiser-windowed sinc 2x filter with wrap
//!    addressing (the texture tiles), albedo filtered in linear light,
//!    normals filtered as unnormalised vectors and renormalised per level,
//!    and the length lost to averaging folded into roughness (Toksvig), so
//!    a bumpy surface gets rougher with distance instead of sparkling;
//! 3. packed as `AH` (albedo sRGB + height) and `NRA` (normal X, Y, roughness,
//!    AO), written per tier size as `<path>_ah.<size>.<fmt>.ktx2`: `rgba` by
//!    our KTX2 writer at half size, `astc` and `bc7` by `ktx create` from
//!    our own levels (`--levels`, no resampling);
//! 4. `kit-textures.json` beside them: the sets as the engine reads them,
//!    plus each layer's mean colour and licence.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::tools::{Tools, s};

/// One map, `ch` floats per texel, square `size`.
#[derive(Debug, Clone, PartialEq)]
pub struct Img {
    pub size: usize,
    pub ch: usize,
    pub px: Vec<f32>,
}

impl Img {
    pub fn new(size: usize, ch: usize) -> Self {
        Self { size, ch, px: vec![0.0; size * size * ch] }
    }

    fn at(&self, x: isize, y: isize, c: usize) -> f32 {
        let n = self.size as isize;
        let (x, y) = (x.rem_euclid(n) as usize, y.rem_euclid(n) as usize);
        self.px[(y * self.size + x) * self.ch + c]
    }
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) }
}

/// Modified Bessel function of the first kind, order 0 (series).
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..32 {
        term *= (x / (2.0 * k as f64)).powi(2);
        sum += term;
    }
    sum
}

/// The 6 taps of the 2x Kaiser-windowed sinc (radius 3 source texels,
/// beta 4), normalised. Tap `t` sits at source offset `t - 2` from `2j`.
pub fn kaiser_taps() -> [f32; 6] {
    const BETA: f64 = 4.0;
    const R: f64 = 3.0;
    let mut w = [0.0f64; 6];
    for (t, wt) in w.iter_mut().enumerate() {
        let d = t as f64 - 2.5; // texel centre to output centre, in source texels
        let win = bessel_i0(BETA * (1.0 - (d / R).powi(2)).max(0.0).sqrt()) / bessel_i0(BETA);
        *wt = sinc(d / 2.0) * win;
    }
    let sum: f64 = w.iter().sum();
    w.map(|x| (x / sum) as f32)
}

/// Halve `img` with the Kaiser filter (separable, wrap addressing).
pub fn downsample(img: &Img) -> Img {
    let taps = kaiser_taps();
    let half = (img.size / 2).max(1);
    if img.size == 1 {
        return img.clone();
    }
    // Rows first: size x half.
    let mut tmp = vec![0.0f32; img.size * half * img.ch];
    for y in 0..img.size {
        for j in 0..half {
            for c in 0..img.ch {
                let mut a = 0.0;
                for (t, w) in taps.iter().enumerate() {
                    a += w * img.at(2 * j as isize + t as isize - 2, y as isize, c);
                }
                tmp[(y * half + j) * img.ch + c] = a;
            }
        }
    }
    let mut out = Img::new(half, img.ch);
    for i in 0..half {
        for x in 0..half {
            for c in 0..img.ch {
                let mut a = 0.0;
                for (t, w) in taps.iter().enumerate() {
                    let y = (2 * i as isize + t as isize - 2).rem_euclid(img.size as isize) as usize;
                    a += w * tmp[(y * half + x) * img.ch + c];
                }
                out.px[(i * half + x) * img.ch + c] = a;
            }
        }
    }
    out
}

/// The full chain from `img` down to 1 x 1.
pub fn chain(img: Img) -> Vec<Img> {
    let mut v = vec![img];
    while v.last().is_some_and(|i| i.size > 1) {
        let next = v.last().map(downsample);
        v.extend(next);
    }
    v
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// A layer's maps at level 0, linear values: albedo rgb (linear light),
/// normal xyz (unit, OpenGL: +y up the image), roughness, AO, height.
#[derive(Debug, Clone)]
pub struct Layer {
    pub albedo: Img,
    pub normal: Img,
    pub rough: Img,
    pub ao: Img,
    pub height: Img,
}

/// Toksvig: the roughness a texel needs once its filtered normal has
/// length `len` (1 = every normal under it agreed), from roughness `r`.
pub fn toksvig(r: f32, len: f32) -> f32 {
    let len = len.clamp(1e-4, 1.0);
    let var = (1.0 - len) / len;
    let a = r * r;
    (a * a + var).min(1.0).sqrt().sqrt()
}

/// One packed level: AH and NRA RGBA8 bytes.
pub struct Packed {
    pub size: usize,
    pub ah: Vec<u8>,
    pub nra: Vec<u8>,
}

fn byte(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Build every level and pack it (level 0 first).
pub fn pack(layer: &Layer) -> Vec<Packed> {
    let alb = chain(layer.albedo.clone());
    let nrm = chain(layer.normal.clone());
    let rgh = chain(layer.rough.clone());
    let ao = chain(layer.ao.clone());
    let hgt = chain(layer.height.clone());
    let mut out = Vec::with_capacity(alb.len());
    for k in 0..alb.len() {
        let n = alb[k].size;
        let mut ah = Vec::with_capacity(n * n * 4);
        let mut nra = Vec::with_capacity(n * n * 4);
        for i in 0..n * n {
            for c in 0..3 {
                ah.push(byte(linear_to_srgb(alb[k].px[i * 3 + c])));
            }
            ah.push(byte(hgt[k].px[i]));
            let v = [nrm[k].px[i * 3], nrm[k].px[i * 3 + 1], nrm[k].px[i * 3 + 2]];
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
            nra.push(byte(v[0] / len * 0.5 + 0.5));
            nra.push(byte(v[1] / len * 0.5 + 0.5));
            let r = rgh[k].px[i].clamp(0.0, 1.0);
            nra.push(byte(if k == 0 { r } else { toksvig(r, len) }));
            nra.push(byte(ao[k].px[i]));
        }
        out.push(Packed { size: n, ah, nra });
    }
    out
}

/// A KTX2 file of RGBA8 levels (level 0 first in `levels`): vkFormat 43
/// (sRGB) or 37, no supercompression, with a basic data format descriptor.
pub fn ktx2_rgba(levels: &[Vec<u8>], size: u32, srgb: bool) -> Vec<u8> {
    let n = levels.len();
    let mut dfd = Vec::new();
    let samples: [(u8, u8); 4] = [(0, 0), (1, 0), (2, 0), (15, if srgb { 0x40 } else { 0 })];
    dfd.extend_from_slice(&(4u32 + 24 + 16 * 4).to_le_bytes());
    dfd.extend_from_slice(&0u32.to_le_bytes()); // vendor 0, descriptor type 0
    dfd.extend_from_slice(&((2u32) | ((24 + 16 * 4) << 16)).to_le_bytes()); // version 2, block size
    dfd.extend_from_slice(&[1, 1, if srgb { 2 } else { 1 }, 0]); // RGBSDA, BT.709, transfer, flags
    dfd.extend_from_slice(&[0, 0, 0, 0]); // 1 x 1 x 1 texel block
    dfd.extend_from_slice(&[4, 0, 0, 0, 0, 0, 0, 0]); // bytes per plane
    for (i, (chan, flags)) in samples.iter().enumerate() {
        dfd.extend_from_slice(&((i as u16) * 8).to_le_bytes());
        dfd.push(7);
        dfd.push(chan | flags);
        dfd.extend_from_slice(&[0, 0, 0, 0]);
        dfd.extend_from_slice(&0u32.to_le_bytes());
        dfd.extend_from_slice(&255u32.to_le_bytes());
    }
    let index_end = 80 + 24 * n;
    let dfd_off = index_end;
    let mut data_off = (dfd_off + dfd.len()).div_ceil(4) * 4;
    let mut out = Vec::new();
    out.extend_from_slice(&[0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A]);
    for v in [if srgb { 43u32 } else { 37 }, 1, size, size, 0, 0, 1, n as u32, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&(dfd_off as u32).to_le_bytes());
    out.extend_from_slice(&(dfd.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 8]); // no key/value data
    out.extend_from_slice(&[0u8; 16]); // no supercompression global data
    // Level data goes smallest first in the file (KTX2 rule); the index
    // lists level 0 first.
    let mut offsets = vec![0usize; n];
    for k in (0..n).rev() {
        offsets[k] = data_off;
        data_off = (data_off + levels[k].len()).div_ceil(4) * 4;
    }
    for k in 0..n {
        let len = levels[k].len() as u64;
        out.extend_from_slice(&(offsets[k] as u64).to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
    }
    out.extend_from_slice(&dfd);
    for k in (0..n).rev() {
        out.resize(offsets[k], 0);
        out.extend_from_slice(&levels[k]);
    }
    out
}

/// An RGBA8 PNG (zlib from flate2, CRC from flate2), for `ktx create`.
pub fn png_rgba(size: usize, px: &[u8]) -> Result<Vec<u8>, String> {
    let mut raw = Vec::with_capacity((size * 4 + 1) * size);
    for row in px.chunks(size * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&raw).map_err(|e| e.to_string())?;
    let z = z.finish().map_err(|e| e.to_string())?;
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |ty: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut crc = flate2::Crc::new();
        crc.update(ty);
        crc.update(data);
        out.extend_from_slice(ty);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc.sum().to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(size as u32).to_be_bytes());
    ihdr.extend_from_slice(&(size as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    Ok(out)
}

// ---- our own procedural layers (tiling) -----------------------------------

fn hash(x: i64, y: i64, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xff_ffff) as f32 / 16_777_215.0
}

/// Value noise with lattice period `p` cells across the tile (tiles).
fn vnoise(u: f32, v: f32, p: i64, seed: u32) -> f32 {
    let (x, y) = (u * p as f32, v * p as f32);
    let (x0, y0) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let g = |i: i64, j: i64| hash(i.rem_euclid(p), j.rem_euclid(p), seed);
    let a = g(x0, y0) + (g(x0 + 1, y0) - g(x0, y0)) * s(fx);
    let b = g(x0, y0 + 1) + (g(x0 + 1, y0 + 1) - g(x0, y0 + 1)) * s(fx);
    a + (b - a) * s(fy)
}

fn fbm(u: f32, v: f32, p: i64, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut per) = (0.0, 0.5, 0.0, p);
    for o in 0..octaves {
        sum += amp * vnoise(u, v, per, seed.wrapping_add(o * 977));
        norm += amp;
        amp *= 0.5;
        per *= 2;
    }
    sum / norm
}

/// The normal of a height field (`strength` texels of slope per unit of
/// height), OpenGL convention (+y up the image), wrap addressing.
fn normals_from_height(h: &Img, strength: f32) -> Img {
    let mut n = Img::new(h.size, 3);
    for y in 0..h.size as isize {
        for x in 0..h.size as isize {
            let dx = (h.at(x + 1, y, 0) - h.at(x - 1, y, 0)) * 0.5 * strength;
            let dy = (h.at(x, y + 1, 0) - h.at(x, y - 1, 0)) * 0.5 * strength;
            let v = [-dx, dy, 1.0];
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let i = (y as usize * h.size + x as usize) * 3;
            n.px[i..i + 3].copy_from_slice(&[v[0] / l, v[1] / l, v[2] / l]);
        }
    }
    n
}

/// Cavity AO: darker where the height is below its blurred surroundings.
fn cavity_ao(h: &Img, radius: isize, depth: f32) -> Img {
    let mut a = Img::new(h.size, 1);
    let r = radius.max(1);
    for y in 0..h.size as isize {
        for x in 0..h.size as isize {
            let mut m = 0.0;
            let mut c = 0.0;
            for j in [-r, 0, r] {
                for i in [-r, 0, r] {
                    m += h.at(x + i, y + j, 0);
                    c += 1.0;
                }
            }
            let d = (m / c - h.at(x, y, 0)).max(0.0);
            a.px[y as usize * h.size + x as usize] = (1.0 - d * depth).clamp(0.2, 1.0);
        }
    }
    a
}

/// One of our recipes at `size`: `rock` (layered sandstone wall),
/// `field` (ploughed soil with grass tufts), `paving` (concrete slabs with
/// joints). All original, all tiling.
pub fn procedural(kind: &str, size: usize, seed: u32) -> Result<Layer, String> {
    let mut albedo = Img::new(size, 3);
    let mut height = Img::new(size, 1);
    let mut rough = Img::new(size, 1);
    let inv = 1.0 / size as f32;
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f32 * inv, y as f32 * inv);
            let i = y * size + x;
            let (col, h, r): ([f32; 3], f32, f32) = match kind {
                "rock" => {
                    let warp = fbm(u, v, 4, 4, seed) - 0.5;
                    // rem_euclid, not fract: fract keeps the sign, so a negative warp
                    // near v = 0 flattened a whole row (a seam line).
                    let band = ((v + warp * 0.12) * 9.0).rem_euclid(1.0);
                    let ledge = (band * 4.0).min(1.0) * (1.0 - ((band - 0.85).max(0.0) * 6.0));
                    let grain = fbm(u, v, 32, 4, seed + 7);
                    let chips = fbm(u, v, 8, 5, seed + 3);
                    let h = (0.55 * ledge + 0.3 * chips + 0.15 * grain).clamp(0.0, 1.0);
                    let tone = 0.75 + 0.25 * (fbm(u, v, 2, 3, seed + 11)) + 0.15 * (band - 0.5);
                    let base = [0.62 * tone, 0.42 * tone, 0.28 * tone];
                    let g = 0.85 + 0.3 * grain;
                    ([base[0] * g, base[1] * g, base[2] * g], h, 0.82 + 0.15 * grain)
                }
                "field" => {
                    let rows = 16.0;
                    let furrow = 0.5 + 0.5 * ((u + 0.03 * (fbm(u, v, 4, 3, seed) - 0.5)) * rows * std::f32::consts::TAU).cos();
                    let clod = fbm(u, v, 16, 4, seed + 5);
                    let tuft = (fbm(u, v, 24, 3, seed + 9) - 0.55).max(0.0) * 4.0 * (1.0 - furrow);
                    let h = (0.6 * furrow + 0.3 * clod + 0.3 * tuft).clamp(0.0, 1.0);
                    let soil = [0.30 + 0.08 * clod, 0.21 + 0.05 * clod, 0.13 + 0.03 * clod];
                    let grass = [0.20, 0.32 + 0.1 * clod, 0.10];
                    let t = tuft.min(1.0);
                    ([soil[0] + (grass[0] - soil[0]) * t, soil[1] + (grass[1] - soil[1]) * t, soil[2] + (grass[2] - soil[2]) * t], h, 0.9 - 0.15 * t)
                }
                "paving" => {
                    let slabs = 4.0;
                    let (su, sv) = (u * slabs, v * slabs);
                    let (fu, fv) = (su.fract(), sv.fract());
                    let edge = fu.min(1.0 - fu).min(fv.min(1.0 - fv));
                    let joint = (1.0 - (edge * 60.0).min(1.0)).powi(2);
                    let id = hash(su.floor() as i64, sv.floor() as i64, seed);
                    let speck = fbm(u, v, 64, 3, seed + 2);
                    let stain = fbm(u, v, 4, 4, seed + 4);
                    let crack = (1.0 - ((fbm(u, v, 8, 4, seed + 6) - 0.5).abs() * 90.0).min(1.0)) * (stain - 0.45).max(0.0) * 3.0;
                    let g = 0.52 + 0.08 * (id - 0.5) + 0.1 * (speck - 0.5) - 0.12 * (stain - 0.5) - 0.25 * joint - 0.2 * crack.min(1.0);
                    let h = (0.85 - 0.7 * joint - 0.3 * crack.min(1.0) + 0.08 * speck).clamp(0.0, 1.0);
                    ([g, g * 0.98, g * 0.95], h, 0.72 + 0.12 * speck + 0.15 * joint)
                }
                _ => return Err(format!("no procedural layer `{kind}` (rock, field, paving)")),
            };
            for (c, v) in col.iter().enumerate() {
                albedo.px[i * 3 + c] = srgb_to_linear(v.clamp(0.0, 1.0));
            }
            height.px[i] = h;
            rough.px[i] = r.clamp(0.05, 1.0);
        }
    }
    let strength = match kind {
        "rock" => 6.0,
        "field" => 4.0,
        _ => 3.0,
    } * size as f32
        / 256.0;
    let normal = normals_from_height(&height, strength);
    let ao = cavity_ao(&height, (size / 128).max(1) as isize, 3.0);
    Ok(Layer { albedo, normal, rough, ao, height })
}

// ---- the command ------------------------------------------------------------

/// What one run wrote.
#[derive(Debug, Default)]
pub struct Done {
    pub files: Vec<PathBuf>,
    pub manifest: PathBuf,
    /// Layers left as they were (inputs and settings unchanged).
    pub skipped: usize,
}

/// Bumped when the converter's output for the same inputs changes, so old
/// stamps stop matching.
const STAMP_VERSION: u32 = 1;

/// Every size a set writes: each tier's size, plus its boot size (W3c)
/// when that is below some tier's size. Ascending.
pub fn set_sizes(set: &Value) -> Vec<usize> {
    let mut sizes: Vec<usize> = set.get("size").and_then(Value::as_object).into_iter().flatten().filter_map(|(_, n)| n.as_u64()).filter(|n| *n > 0).map(|n| n as usize).collect();
    let boot = set.get("boot").and_then(Value::as_u64).unwrap_or(0) as usize;
    if boot > 0 && sizes.iter().any(|n| *n > boot) {
        sizes.push(boot);
    }
    sizes.sort_unstable();
    sizes.dedup();
    sizes
}

/// The map names Poly Haven and ambientCG downloads use, per input
/// (lowercase, matched against the end of the file name before the
/// resolution tag and extension). `normal_dx` is flipped to OpenGL; `arm`
/// is Poly Haven's AO / roughness / metal pack.
const MAP_NAMES: &[(&str, &[&str])] = &[
    ("albedo", &["diff", "diffuse", "color", "col", "albedo", "basecolor", "base_color"]),
    ("normal", &["nor_gl", "normalgl", "normal_gl", "normal"]),
    ("normal_dx", &["nor_dx", "normaldx", "normal_dx"]),
    ("roughness", &["rough", "roughness"]),
    ("ao", &["ao", "ambientocclusion", "ambient_occlusion", "occlusion"]),
    ("height", &["disp", "displacement", "height"]),
    ("arm", &["arm"]),
];

/// The image files sharp reads.
const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "tif", "tiff"];

/// Which input a file is (`albedo`, `normal`, ...), from its name:
/// `rock_wall_diff_2k.jpg`, `Rock023_2K-JPG_NormalGL.jpg`.
pub fn map_kind(file: &str) -> Option<&'static str> {
    let lower = file.to_ascii_lowercase();
    let (stem, ext) = lower.rsplit_once('.')?;
    if !IMAGE_EXT.contains(&ext) {
        return None;
    }
    // Words after the first, minus a resolution tag (1k, 2k, 4k-jpg ...).
    let words: Vec<&str> = stem.split(['_', '-', ' ', '.']).filter(|w| !w.is_empty()).collect();
    let is_res = |w: &str| (w.len() > 1 && w.ends_with('k') && w[..w.len() - 1].chars().all(|c| c.is_ascii_digit())) || matches!(w, "jpg" | "png" | "jpeg" | "exr");
    let words: Vec<&str> = words.into_iter().filter(|w| !is_res(w)).collect();
    for take in [2usize, 1] {
        if words.len() <= take {
            continue;
        }
        let tail = words[words.len() - take..].join("_");
        let tail_joined = words[words.len() - take..].concat();
        for (kind, names) in MAP_NAMES {
            if names.iter().any(|n| *n == tail || *n == tail_joined) {
                return Some(kind);
            }
        }
    }
    None
}

/// Find a layer's maps in a downloaded folder (Poly Haven or ambientCG
/// names). When one map comes at several resolutions, the largest file
/// wins. Needs an albedo; OpenGL normals win over DirectX ones.
pub fn find_maps(dir: &Path) -> Result<Value, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut best: Vec<(&'static str, u64, String)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(kind) = map_kind(&name) else { continue };
        let len = e.metadata().map_or(0, |m| m.len());
        match best.iter_mut().find(|(k, _, _)| *k == kind) {
            Some(b) if b.1 >= len => {}
            Some(b) => *b = (kind, len, name),
            None => best.push((kind, len, name)),
        }
    }
    let rel = |n: &str| s(&dir.join(n));
    let mut maps = serde_json::Map::new();
    for (kind, _, name) in &best {
        if *kind == "normal_dx" && best.iter().any(|(k, _, _)| *k == "normal") {
            continue;
        }
        maps.insert((*kind).to_string(), json!(rel(name)));
    }
    if !maps.contains_key("albedo") {
        let seen: Vec<&str> = best.iter().map(|(k, _, _)| *k).collect();
        return Err(format!("{}: no albedo map (a file ending _diff, _color or _albedo; found {seen:?})", dir.display()));
    }
    Ok(Value::Object(maps))
}

/// A layer's stamp: its settings, its sizes and formats, and every input
/// file's length and change time (FNV hex).
fn layer_stamp(layer: &Value, maps: Option<&Value>, sizes: &[usize], formats: &[String], base: &Path) -> String {
    let mut key = format!("v{STAMP_VERSION}|{layer}|{sizes:?}|{formats:?}|{}", maps.map(Value::to_string).unwrap_or_default());
    for p in maps.and_then(Value::as_object).into_iter().flatten().filter_map(|(_, v)| v.as_str()) {
        let m = std::fs::metadata(base.join(p)).ok();
        let t = m.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        key.push_str(&format!("|{p}:{}:{t}", m.map_or(0, |m| m.len())));
    }
    evani_guide::fnv_hex(key.as_bytes())
}

/// The layer's mean colour when its stamp matches and every output is
/// there (nothing to redo).
fn up_to_date(stamp_file: &Path, stamp: &str, outputs: &[PathBuf]) -> Option<Value> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(stamp_file).ok()?).ok()?;
    (v.get("stamp").and_then(Value::as_str) == Some(stamp) && outputs.iter().all(|o| o.is_file())).then(|| v.get("mean").cloned().unwrap_or(Value::Null))
}

/// Where `convert-textures --app-dir <app>` reads and writes: the layer
/// list (`<app>/layers.json`, else the game folder's, two up, as
/// `examples/ground-textures` keeps it) and the web root `<app>/assets`.
pub fn app_dir_paths(app: &Path) -> Result<(PathBuf, PathBuf), String> {
    let candidates = [app.join("layers.json"), app.join("..").join("..").join("layers.json")];
    let spec = candidates.iter().find(|p| p.is_file()).ok_or_else(|| format!("no layers.json in {} or its game folder (docs/GROUND_TEXTURES.md has the format)", app.display()))?;
    Ok((spec.clone(), app.join("assets")))
}

/// Decode photo maps with sharp (resized to `size`) into a [`Layer`].
fn photo(tools: &Tools, work: &Path, maps: &Value, size: usize, base: &Path) -> Result<Layer, String> {
    const SCRIPT: &str = "import { createRequire } from 'node:module';\nimport fs from 'node:fs';\nimport path from 'node:path';\nconst [nm, job] = process.argv.slice(2);\nconst sharp = createRequire(path.join(nm, 'evani-convert.cjs'))('sharp');\nsharp.concurrency(1); sharp.cache(false);\nconst j = JSON.parse(fs.readFileSync(job, 'utf8'));\nfor (const m of j.maps) {\n  const b = await sharp(m.src).resize(j.size, j.size, { fit: 'fill', kernel: 'lanczos3' }).ensureAlpha().raw().toBuffer();\n  fs.writeFileSync(m.out, b);\n}\n";
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let script = work.join("decode.mjs");
    std::fs::write(&script, SCRIPT).map_err(|e| e.to_string())?;
    let mut list = Vec::new();
    for key in ["albedo", "normal", "normal_dx", "roughness", "ao", "height", "arm"] {
        if let Some(src) = maps.get(key).and_then(Value::as_str) {
            list.push(json!({ "src": s(&base.join(src)), "out": s(&work.join(format!("{key}.raw"))) }));
        }
    }
    let job = work.join("decode.json");
    std::fs::write(&job, json!({ "size": size, "maps": list }).to_string()).map_err(|e| e.to_string())?;
    tools.node_script(&script, &[&s(&job)], "decode layer maps (sharp)")?;
    let read = |key: &str| -> Option<Vec<u8>> { std::fs::read(work.join(format!("{key}.raw"))).ok().filter(|b| b.len() == size * size * 4) };
    let albedo_b = read("albedo").ok_or("a photo layer needs \"albedo\"")?;
    let mut l = Layer { albedo: Img::new(size, 3), normal: Img::new(size, 3), rough: Img::new(size, 1), ao: Img::new(size, 1), height: Img::new(size, 1) };
    // An OpenGL normal wins; a DirectX one is flipped (green down the image).
    let (nb, flip_y) = match read("normal") {
        Some(b) => (Some(b), false),
        None => (read("normal_dx"), true),
    };
    let (rb, ab, hb, arm) = (read("roughness"), read("ao"), read("height"), read("arm"));
    decode_into(&mut l, &albedo_b, Maps { normal: nb.as_deref(), flip_y, rough: rb.as_deref(), ao: ab.as_deref(), height: hb.as_deref(), arm: arm.as_deref() });
    Ok(l)
}

/// Decoded RGBA8 inputs of a photo layer (all `size * size * 4` bytes).
#[derive(Default, Clone, Copy)]
pub struct Maps<'a> {
    pub normal: Option<&'a [u8]>,
    /// The normal is DirectX (Y down): flip it to OpenGL.
    pub flip_y: bool,
    pub rough: Option<&'a [u8]>,
    pub ao: Option<&'a [u8]>,
    pub height: Option<&'a [u8]>,
    /// Poly Haven's ARM pack: AO in R, roughness in G (used where the
    /// separate map is missing).
    pub arm: Option<&'a [u8]>,
}

/// Fill a layer from decoded maps. Missing maps get flat defaults: a flat
/// normal, roughness 0.8, AO 1, height 0.5.
pub fn decode_into(l: &mut Layer, albedo: &[u8], m: Maps) {
    let n_px = l.albedo.size * l.albedo.size;
    let ch = |b: Option<&[u8]>, i: usize, c: usize| b.and_then(|b| b.get(i * 4 + c)).map(|v| f32::from(*v) / 255.0);
    for i in 0..n_px {
        for c in 0..3 {
            l.albedo.px[i * 3 + c] = srgb_to_linear(f32::from(albedo.get(i * 4 + c).copied().unwrap_or(0)) / 255.0);
        }
        let mut n = match m.normal {
            Some(_) => [ch(m.normal, i, 0).unwrap_or(0.5) * 2.0 - 1.0, ch(m.normal, i, 1).unwrap_or(0.5) * 2.0 - 1.0, ch(m.normal, i, 2).unwrap_or(1.0) * 2.0 - 1.0],
            None => [0.0, 0.0, 1.0],
        };
        if m.flip_y {
            n[1] = -n[1];
        }
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
        l.normal.px[i * 3..i * 3 + 3].copy_from_slice(&[n[0] / len, n[1] / len, n[2] / len]);
        l.rough.px[i] = ch(m.rough, i, 0).or_else(|| ch(m.arm, i, 1)).unwrap_or(0.8);
        l.ao.px[i] = ch(m.ao, i, 0).or_else(|| ch(m.arm, i, 0)).unwrap_or(1.0);
        l.height.px[i] = ch(m.height, i, 0).unwrap_or(0.5);
    }
}

/// Encode one tier size of one map with `ktx create` from our levels.
fn encode(tools: &Tools, work: &Path, levels: &[(usize, &[u8])], fmt: &str, srgb: bool, out: &Path) -> Result<(), String> {
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let mut inputs = Vec::new();
    for (k, (n, px)) in levels.iter().enumerate() {
        let p = work.join(format!("level{k}.png"));
        std::fs::write(&p, png_rgba(*n, px)?).map_err(|e| e.to_string())?;
        inputs.push(s(&p));
    }
    let tf = if srgb { "srgb" } else { "linear" };
    let lv = levels.len().to_string();
    let mut a: Vec<String> = vec!["create".into()];
    let target = if fmt == "astc" { out.to_path_buf() } else { out.with_extension("uastc.ktx2") };
    if fmt == "astc" {
        a.extend(["--format".into(), if srgb { "ASTC_4x4_SRGB_BLOCK" } else { "ASTC_4x4_UNORM_BLOCK" }.into(), "--astc-quality".into(), "medium".into()]);
    } else {
        a.extend(["--format".into(), if srgb { "R8G8B8A8_SRGB" } else { "R8G8B8A8_UNORM" }.into(), "--encode".into(), "uastc".into(), "--uastc-quality".into(), "2".into()]);
    }
    a.extend(["--assign-tf".into(), tf.into(), "--levels".into(), lv, "--threads".into(), "1".into()]);
    a.extend(inputs);
    a.push(s(&target));
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    tools.ktx(&a, &format!("ktx create {fmt} {}", out.display()))?;
    if fmt == "bc7" {
        tools.ktx(&["transcode", "--target", "bc7", &s(&target), &s(out)], &format!("ktx transcode BC7 {}", out.display()))?;
        let _ = std::fs::remove_file(target);
    }
    Ok(())
}

/// Run `layers.json` into `out` (the game's web root, `<app>/assets`).
/// Relative photo paths and folders start from the spec's folder. A layer
/// whose inputs and settings match its last run is left alone unless
/// `force`.
pub fn run(root: &Path, spec_path: &Path, out: &Path, force: bool) -> Result<Done, String> {
    let text = std::fs::read_to_string(spec_path).map_err(|e| format!("{}: {e}", spec_path.display()))?;
    let spec: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", spec_path.display()))?;
    let base = spec_path.parent().unwrap_or(Path::new("."));
    let formats: Vec<String> = spec.get("formats").and_then(Value::as_array).map_or_else(|| vec!["astc".into(), "bc7".into(), "rgba".into()], |a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
    // Photo layers need the whole tool set (sharp lives in its npm tree);
    // ASTC and BC7 only need the pinned `ktx`.
    let photos = spec.get("sets").and_then(Value::as_array).into_iter().flatten().flat_map(|s| s.get("layers").and_then(Value::as_array).into_iter().flatten()).any(|l| l.get("source").is_some_and(|src| src.get("maps").is_some() || src.get("folder").is_some()));
    let tools = if photos {
        Some(Tools::locate(root)?)
    } else if formats.iter().any(|f| f != "rgba") {
        let ktx = evani_guide::extools::locate(root, "ktx")?;
        Some(Tools { gltfpack: PathBuf::new(), ktx, gltf_transform: PathBuf::new(), node_modules: PathBuf::new(), node: PathBuf::new() })
    } else {
        None
    };
    let work = root.join(".evani").join("tmp").join("convert-textures");
    let mut done = Done::default();
    let mut manifest = spec.clone();
    for (si, set) in spec.get("sets").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let sizes = set_sizes(set);
        let Some(&top) = sizes.last() else { continue };
        for (li, l) in set.get("layers").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let path = l.get("path").and_then(Value::as_str).ok_or("every layer needs a \"path\"")?;
            let name = l.get("name").and_then(Value::as_str).unwrap_or(path);
            let src = l.get("source").ok_or_else(|| format!("layer {name}: no \"source\" (\"procedural\", \"maps\" or \"folder\")"))?;
            let lw = work.join(format!("s{si}-{name}"));
            // A folder of downloaded maps (Poly Haven or ambientCG names).
            let found = match src.get("folder").and_then(Value::as_str) {
                Some(dir) => Some(find_maps(&base.join(dir)).map_err(|e| format!("layer {name}: {e}"))?),
                None => None,
            };
            let maps = found.as_ref().or_else(|| src.get("maps"));
            // Skip a layer whose inputs and settings haven't changed (W3d).
            let outputs: Vec<PathBuf> = sizes.iter().flat_map(|n| formats.iter().flat_map(move |f| ["_ah", "_nra"].map(|m| format!("{path}{m}.{n}.{f}.ktx2")))).map(|r| out.join(r)).collect();
            let stamp = layer_stamp(l, maps, &sizes, &formats, base);
            let stamp_file = work.join("stamps").join(format!("{}.json", evani_guide::fnv_hex(s(&out.join(path)).as_bytes())));
            if !force
                && let Some(mean) = up_to_date(&stamp_file, &stamp, &outputs)
            {
                manifest["sets"][si]["layers"][li]["mean"] = mean;
                if let Some(o) = manifest["sets"][si]["layers"][li].as_object_mut() {
                    o.remove("source");
                }
                println!("  up to date {name} ({} files)", outputs.len());
                done.skipped += 1;
                continue;
            }
            let layer = match (src.get("procedural").and_then(Value::as_str), maps) {
                (Some(kind), _) => procedural(kind, top, src.get("seed").and_then(Value::as_u64).unwrap_or(1) as u32)?,
                (None, Some(maps)) => photo(tools.as_ref().ok_or("photo layers need the convert tools")?, &lw, maps, top, base)?,
                _ => return Err(format!("layer {name}: \"source\" needs \"procedural\", \"maps\" or \"folder\"")),
            };
            let packed = pack(&layer);
            // The mean colour (the 1 x 1 level), for the far fade.
            let mean = packed.last().map_or(json!(null), |last| json!([last.ah[0], last.ah[1], last.ah[2]]));
            manifest["sets"][si]["layers"][li]["mean"] = mean.clone();
            for &n in &sizes {
                for f in &formats {
                    let first = if f == "rgba" { (n / 2).max(1) } else { n };
                    let lv: Vec<&Packed> = packed.iter().filter(|p| p.size <= first).collect();
                    for (map, srgb) in [("_ah", true), ("_nra", false)] {
                        let rel = format!("{path}{map}.{n}.{f}.ktx2");
                        let file = out.join(&rel);
                        if let Some(d) = file.parent() {
                            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                        }
                        let levels: Vec<(usize, &[u8])> = lv.iter().map(|p| (p.size, if srgb { p.ah.as_slice() } else { p.nra.as_slice() })).collect();
                        if f == "rgba" {
                            let bytes: Vec<Vec<u8>> = levels.iter().map(|(_, b)| b.to_vec()).collect();
                            std::fs::write(&file, ktx2_rgba(&bytes, first as u32, srgb)).map_err(|e| e.to_string())?;
                        } else {
                            let t = tools.as_ref().ok_or("ASTC and BC7 need the convert tools")?;
                            encode(t, &lw.join(format!("{n}{map}.{f}")), &levels, f, srgb, &file)?;
                        }
                        println!("  wrote {rel}");
                        done.files.push(file);
                    }
                }
            }
            if let Some(o) = manifest["sets"][si]["layers"][li].as_object_mut() {
                o.remove("source");
            }
            if let Some(d) = stamp_file.parent() {
                std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
            }
            std::fs::write(&stamp_file, json!({ "stamp": stamp, "mean": mean }).to_string()).map_err(|e| e.to_string())?;
        }
    }
    done.manifest = out.join(evani_guide::groundtex::MANIFEST);
    std::fs::write(&done.manifest, serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kaiser_filter_keeps_flat_maps_flat_and_halves_the_size() {
        let t = kaiser_taps();
        assert!((t.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(t[2] > 0.3 && t[0] < 0.0, "{t:?}");
        let mut img = Img::new(16, 1);
        img.px.iter_mut().for_each(|p| *p = 0.4);
        let d = downsample(&img);
        assert_eq!(d.size, 8);
        assert!(d.px.iter().all(|p| (p - 0.4).abs() < 1e-5));
        assert_eq!(chain(img).len(), 5);
    }

    /// Wrap addressing: a tiling stripe pattern stays tiling (the last
    /// column filters with the first).
    #[test]
    fn the_filter_wraps() {
        let mut img = Img::new(8, 1);
        for y in 0..8 {
            img.px[y * 8] = 1.0;
        }
        let d = downsample(&img);
        // Output 3 reaches column 0 only through the wrap (as column 8).
        assert!(d.px[3] > 0.05 && d.px[0] > d.px[3], "{:?}", &d.px[..4]);
        assert!(d.px[2].abs() < 1e-6);
    }

    /// Toksvig: flat normals keep their roughness at every level; bumpy
    /// ones get rougher further down the chain, and every normal stays unit.
    #[test]
    fn toksvig_folds_normal_variance_into_roughness() {
        assert!((toksvig(0.3, 1.0) - 0.3).abs() < 1e-5);
        assert!(toksvig(0.3, 0.8) > 0.5);
        assert!(toksvig(0.3, 0.8) <= 1.0);
        let size = 32;
        let flat = Layer { albedo: Img::new(size, 3), normal: { let mut n = Img::new(size, 3); n.px.chunks_mut(3).for_each(|c| c[2] = 1.0); n }, rough: { let mut r = Img::new(size, 1); r.px.iter_mut().for_each(|p| *p = 0.3); r }, ao: Img::new(size, 1), height: Img::new(size, 1) };
        let mut bumpy = flat.clone();
        for (i, c) in bumpy.normal.px.chunks_mut(3).enumerate() {
            let s = if (i % size + i / size) % 2 == 0 { 0.6 } else { -0.6 };
            c.copy_from_slice(&[s, 0.0, 0.8]);
        }
        let (pf, pb) = (pack(&flat), pack(&bumpy));
        assert_eq!(pf.len(), 6);
        let r = |p: &Packed| p.nra[2];
        assert_eq!(r(&pf[0]), r(&pf[3]), "flat keeps its roughness");
        assert!(r(&pb[3]) > r(&pb[0]) + 40, "bumpy roughens: {} -> {}", r(&pb[0]), r(&pb[3]));
        for p in &pb {
            for c in p.nra.chunks(4) {
                let (x, y) = (f32::from(c[0]) / 127.5 - 1.0, f32::from(c[1]) / 127.5 - 1.0);
                assert!(x * x + y * y <= 1.02, "renormalised");
            }
        }
    }

    #[test]
    fn rgba_ktx2_passes_the_engine_check() {
        let layer = procedural("paving", 32, 1).unwrap_or_else(|e| panic!("{e}"));
        let packed = pack(&layer);
        // `rgba` for size 64 holds 32 x 32 with its full chain.
        let levels: Vec<Vec<u8>> = packed.iter().map(|p| p.ah.clone()).collect();
        let k = ktx2_rgba(&levels, 32, true);
        assert_eq!(evani_guide::groundtex::file_problem(&k, "rgba", 64), Ok(()));
        assert!(evani_guide::groundtex::file_problem(&k, "rgba", 32).is_err());
    }

    #[test]
    fn procedural_layers_tile_and_have_detail() {
        for kind in ["rock", "field", "paving"] {
            let l = procedural(kind, 64, 3).unwrap_or_else(|e| panic!("{e}"));
            let h = &l.height;
            // Tiling: the seam (last column to first) is no bigger a step than
            // the steps inside the tile.
            // Tiling both ways: each seam (last column to first, last row to
            // first) is no bigger a step than the steps inside the tile.
            let col = |a: usize, b: usize| (0..64).map(|y| (h.px[y * 64 + a] - h.px[y * 64 + b]).abs()).sum::<f32>();
            let row = |a: usize, b: usize| (0..64).map(|x| (h.px[a * 64 + x] - h.px[b * 64 + x]).abs()).sum::<f32>();
            let inner_c: f32 = (0..63).map(|x| col(x, x + 1)).fold(0.0, f32::max);
            let inner_r: f32 = (0..63).map(|y| row(y, y + 1)).fold(0.0, f32::max);
            assert!(col(63, 0) <= inner_c * 1.5 + 1e-3, "{kind} column seam");
            assert!(row(63, 0) <= inner_r * 1.5 + 1e-3, "{kind} row seam");
            // No flattened rows or columns (the old rock seam): every row's
            // mean height is within the tile's range of row means.
            let means: Vec<f32> = (0..64).map(|y| (0..64).map(|x| h.px[y * 64 + x]).sum::<f32>() / 64.0).collect();
            let avg = means.iter().sum::<f32>() / 64.0;
            // (Paving has real low rows: its slab joints.)
            assert!(kind == "paving" || means.iter().all(|m| (m - avg).abs() < 0.35), "{kind} has a flattened row: {means:?}");
            let (mn, mx) = h.px.iter().fold((1.0f32, 0.0f32), |(a, b), &p| (a.min(p), b.max(p)));
            assert!(mx - mn > 0.2, "{kind} has relief");
        }
        assert!(procedural("lava", 8, 1).is_err());
    }

    #[test]
    fn poly_haven_and_ambientcg_names_are_recognised() {
        let cases = [
            ("cliff_side_diff_2k.jpg", Some("albedo")),
            ("cliff_side_nor_gl_2k.png", Some("normal")),
            ("cliff_side_nor_dx_4k.png", Some("normal_dx")),
            ("cliff_side_rough_2k.jpg", Some("roughness")),
            ("cliff_side_arm_1k.png", Some("arm")),
            ("cliff_side_disp_2k.png", Some("height")),
            ("cliff_side_ao_2k.jpg", Some("ao")),
            ("Rock023_2K-JPG_Color.jpg", Some("albedo")),
            ("Rock023_2K-JPG_NormalGL.jpg", Some("normal")),
            ("Rock023_2K-JPG_NormalDX.jpg", Some("normal_dx")),
            ("Rock023_2K-JPG_Roughness.jpg", Some("roughness")),
            ("Rock023_2K-JPG_AmbientOcclusion.jpg", Some("ao")),
            ("Rock023_2K-JPG_Displacement.jpg", Some("height")),
            ("cliff_side_diff_2k.exr", None),
            ("readme.txt", None),
            ("diff.png", None),
        ];
        for (f, want) in cases {
            assert_eq!(map_kind(f), want, "{f}");
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("evani-convert-groundtex-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn a_download_folder_gives_its_maps_largest_first_and_gl_over_dx() {
        let d = tmp("folder");
        for (f, n) in [("rock_diff_1k.jpg", 10), ("rock_diff_2k.jpg", 40), ("rock_nor_dx_2k.png", 5), ("rock_nor_gl_2k.png", 5), ("rock_arm_2k.png", 5), ("notes.txt", 1)] {
            let _ = std::fs::write(d.join(f), vec![0u8; n]);
        }
        let m = find_maps(&d).unwrap_or_else(|e| panic!("{e}"));
        let get = |k: &str| m.get(k).and_then(Value::as_str).map(|p| Path::new(p).file_name().map(|f| f.to_string_lossy().into_owned()));
        assert_eq!(get("albedo"), Some(Some("rock_diff_2k.jpg".into())));
        assert_eq!(get("normal"), Some(Some("rock_nor_gl_2k.png".into())));
        assert!(m.get("normal_dx").is_none(), "GL wins over DX");
        assert!(m.get("arm").is_some());
        let _ = std::fs::remove_file(d.join("rock_diff_1k.jpg"));
        let _ = std::fs::remove_file(d.join("rock_diff_2k.jpg"));
        assert!(find_maps(&d).is_err_and(|e| e.contains("no albedo")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn directx_normals_flip_and_arm_fills_roughness_and_ao() {
        let mut l = Layer { albedo: Img::new(1, 3), normal: Img::new(1, 3), rough: Img::new(1, 1), ao: Img::new(1, 1), height: Img::new(1, 1) };
        // A normal tilted +Y in DirectX (green 255) is -Y in OpenGL.
        let dx = [128u8, 255, 128, 255];
        let arm = [51u8, 204, 0, 255];
        decode_into(&mut l, &[255, 0, 0, 255], Maps { normal: Some(&dx), flip_y: true, arm: Some(&arm), ..Default::default() });
        assert!(l.normal.px[1] < -0.5, "{:?}", l.normal.px);
        assert!((l.rough.px[0] - 0.8).abs() < 0.01 && (l.ao.px[0] - 0.2).abs() < 0.01);
        assert!((l.albedo.px[0] - 1.0).abs() < 1e-4 && l.height.px[0] == 0.5);
        // A separate roughness map wins over the pack.
        decode_into(&mut l, &[0; 4], Maps { rough: Some(&[25, 0, 0, 255]), arm: Some(&arm), ..Default::default() });
        assert!(l.rough.px[0] < 0.11 && l.normal.px[2] > 0.99);
    }

    #[test]
    fn boot_sizes_are_written_when_below_a_tier() {
        let set = json!({ "boot": 64, "size": { "low": 0, "medium": 256, "high": 512 } });
        assert_eq!(set_sizes(&set), vec![64, 256, 512]);
        assert_eq!(set_sizes(&json!({ "boot": 512, "size": { "high": 512 } })), vec![512]);
        assert_eq!(set_sizes(&json!({ "size": { "high": 0 } })), Vec::<usize>::new());
    }

    /// W3d: a second run with nothing changed converts nothing and keeps
    /// the manifest (mean colours included); a changed layer is redone.
    #[test]
    fn unchanged_layers_are_skipped() {
        let d = tmp("skip");
        let root = d.join("root");
        let out = d.join("app").join("assets");
        let spec = d.join("layers.json");
        let write_spec = |seed: u32| {
            let v = json!({ "formats": ["rgba"], "sets": [{ "set": 0, "boot": 16, "size": { "high": 64 },
                "layers": [{ "name": "a", "path": "g/a", "source": { "procedural": "rock", "seed": 1 } },
                           { "name": "b", "path": "g/b", "source": { "procedural": "paving", "seed": seed } }] }] });
            let _ = std::fs::write(&spec, v.to_string());
        };
        write_spec(2);
        let first = run(&root, &spec, &out, false).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((first.files.len(), first.skipped), (8, 0), "2 layers x (16 + 64) x 2 maps");
        assert!(out.join("g/a_ah.16.rgba.ktx2").is_file());
        let manifest = std::fs::read_to_string(&first.manifest).unwrap_or_default();
        let again = run(&root, &spec, &out, false).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((again.files.len(), again.skipped), (0, 2));
        assert_eq!(std::fs::read_to_string(&again.manifest).unwrap_or_default(), manifest, "same manifest, means kept");
        write_spec(3);
        let changed = run(&root, &spec, &out, false).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((changed.files.len(), changed.skipped), (4, 1));
        let forced = run(&root, &spec, &out, true).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(forced.skipped, 0);
        // A deleted output is remade.
        let _ = std::fs::remove_file(out.join("g/a_nra.64.rgba.ktx2"));
        assert_eq!(run(&root, &spec, &out, false).map(|d| d.skipped).ok(), Some(1));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn app_dir_finds_the_layer_list() {
        let d = tmp("appdir");
        let app = d.join("apps").join("g");
        let _ = std::fs::create_dir_all(&app);
        assert!(app_dir_paths(&app).is_err());
        let _ = std::fs::write(d.join("layers.json"), "{}");
        assert_eq!(app_dir_paths(&app).map(|p| p.1).ok(), Some(app.join("assets")));
        let _ = std::fs::write(app.join("layers.json"), "{}");
        assert_eq!(app_dir_paths(&app).map(|p| p.0).ok(), Some(app.join("layers.json")), "the app's own list wins");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn png_has_the_right_header() {
        let p = png_rgba(2, &[0; 16]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(&p[1..4], b"PNG");
        assert_eq!(&p[12..16], b"IHDR");
    }
}
