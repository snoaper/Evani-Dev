//! Clay lumps and yarn ridges (WP3, docs/studio/WORK_PACKAGES.md): the
//! viewer's deforms (apps/material-view/src/treat.rs `lump`, `ridge`, the
//! same fields and edge lengths) fed the asset's own source. Each vertex
//! moves along its welded normal by a field of its position, so split
//! corners move together and the surface stays closed. Deterministic: the
//! field is offset by a hash of the asset id (every horse lumps the same
//! way, a horse and a cow differently), never the clock. Each part moves
//! in proportion to its own width (a region piece's cross-section over
//! [`FULL_WIDTH`], at least [`MIN_AMOUNT`]), so a lump can't pinch a leg off
//! or swell a hoof into a ball.

use std::collections::HashMap;

use evani_studio::assets::AssetRow;
use evani_studio::geo::{GeometryGenerator, Passthrough, SourceMesh, StyledMesh};
use evani_studio::styles::{Geometry, GeometryKind};

use crate::math::{V3, add, cross, dot, fnv1a, len, normalize, scale, sub};
use crate::voxel::{piece_width, region_pieces};

/// Mean edge the clay lumps need.
pub const LUMP_EDGE: f32 = 0.07;
/// Mean edge the knit ridges need (their rows are about 0.17 apart).
pub const RIDGE_EDGE: f32 = 0.035;
/// The most triangles refining may reach.
pub const MAX_TRIS: usize = 40_000;
/// A part this wide (radius-1 units) or wider takes the whole amount.
pub const FULL_WIDTH: f32 = 0.4;
/// The least share of the amount a narrow part takes.
pub const MIN_AMOUNT: f32 = 0.15;

#[derive(Debug, Clone, Copy, Default)]
pub struct Lumpy;

impl GeometryGenerator for Lumpy {
    fn kind(&self) -> GeometryKind {
        GeometryKind::Lumpy
    }

    fn generate(&self, src: &SourceMesh, asset: &AssetRow, geometry: &Geometry) -> Result<StyledMesh, String> {
        let Geometry::Lumpy { amount } = *geometry else {
            return Err(format!("lumps can't build `{}`", geometry.kind().name()));
        };
        displaced(src, asset, LUMP_EDGE, &|p| amount * lump(p))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Ridges;

impl GeometryGenerator for Ridges {
    fn kind(&self) -> GeometryKind {
        GeometryKind::Ridges
    }

    fn generate(&self, src: &SourceMesh, asset: &AssetRow, geometry: &Geometry) -> Result<StyledMesh, String> {
        let Geometry::Ridges { amount } = *geometry else {
            return Err(format!("ridges can't build `{}`", geometry.kind().name()));
        };
        displaced(src, asset, RIDGE_EDGE, &|p| amount * ridge(p))
    }
}

/// The asset's field offset (0 to 100 on each axis) from its id.
pub fn seed_offset(asset: &AssetRow) -> V3 {
    let h = fnv1a(asset.id.as_bytes());
    [0, 21, 42].map(|s| ((h >> s) & 0x1f_ffff) as f32 / 0x1f_ffff as f32 * 100.0)
}

/// Refine, push each welded position along its normal by `field`, then
/// cut into parts per region.
fn displaced(src: &SourceMesh, asset: &AssetRow, edge: f32, field: &dyn Fn(V3) -> f32) -> Result<StyledMesh, String> {
    src.validate(asset.regions.len())?;
    let mut m = refined(src.clone(), edge);
    let off = seed_offset(asset);
    let weld: Vec<[i32; 3]> = m.positions.iter().map(|p| key(*p)).collect();
    // Per welded position: the area-weighted normal and the smallest
    // amount any vertex there may move.
    let mut normal: HashMap<[i32; 3], V3> = HashMap::new();
    let mut amount: HashMap<[i32; 3], f32> = HashMap::new();
    for t in m.indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|v| m.positions[v as usize]);
        let n = cross(sub(b, a), sub(c, a));
        for &v in t {
            let e = normal.entry(weld[v as usize]).or_insert([0.0; 3]);
            *e = add(*e, n);
        }
    }
    let share = part_share(&m);
    for (v, w) in weld.iter().enumerate() {
        let e = amount.entry(*w).or_insert(1.0);
        *e = e.min(share[v]);
    }
    let old_normals = m.normals.clone();
    for (v, p) in m.positions.iter_mut().enumerate() {
        let w = weld[v];
        let d = normal.get(&w).map_or(old_normals[v], |n| normalize(*n, old_normals[v]));
        let h = field(add(*p, off)) * amount.get(&w).copied().unwrap_or(1.0);
        *p = add(*p, scale(d, h));
    }
    renormal(&mut m);
    Passthrough.generate(&m, asset, &Geometry::None)
}

/// Per vertex: the share of the amount its part takes (module docs).
fn part_share(m: &SourceMesh) -> Vec<f32> {
    let mut share = vec![1.0f32; m.positions.len()];
    let tris = m.indices.as_chunks::<3>().0;
    for (_, piece) in region_pieces(m, &|_| true) {
        let k = (piece_width(m, &piece) / FULL_WIDTH).clamp(MIN_AMOUNT, 1.0);
        for &t in &piece {
            for &v in &tris[t] {
                share[v as usize] = share[v as usize].min(k);
            }
        }
    }
    share
}

fn key(p: V3) -> [i32; 3] {
    p.map(|x| (x * 1e5).round() as i32)
}

/// Normals again from the faces round each position, keeping hard edges:
/// a face counts for a corner only if it roughly agrees with the old one.
fn renormal(m: &mut SourceMesh) {
    let mut faces: HashMap<[i32; 3], Vec<V3>> = HashMap::new();
    for t in m.indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|v| m.positions[v as usize]);
        let n = cross(sub(b, a), sub(c, a));
        for &v in t {
            faces.entry(key(m.positions[v as usize])).or_default().push(n);
        }
    }
    let normals = m
        .positions
        .iter()
        .zip(&m.normals)
        .map(|(p, old)| {
            let mut s = [0.0; 3];
            for &n in faces.get(&key(*p)).map_or(&[][..], Vec::as_slice) {
                if dot(normalize(n, *old), *old) > 0.5 {
                    s = add(s, n);
                }
            }
            normalize(s, *old)
        })
        .collect();
    m.normals = normals;
}

fn mean_edge(m: &SourceMesh) -> f32 {
    let (mut sum, mut n) = (0.0f64, 0u32);
    for t in m.indices.as_chunks::<3>().0 {
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            sum += f64::from(len(sub(m.positions[t[i] as usize], m.positions[t[j] as usize])));
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { (sum / f64::from(n)) as f32 }
}

/// Split every triangle in four until the mean edge is at most `edge` (or
/// the next split would pass [`MAX_TRIS`]).
fn refined(mut m: SourceMesh, edge: f32) -> SourceMesh {
    while mean_edge(&m) > edge && m.indices.len() / 3 * 4 <= MAX_TRIS {
        m = subdivided(&m);
    }
    m
}

/// Every triangle split in four at its edges' midpoints, shared by welded
/// position, so a seam of split corners gets one midpoint and no crack.
fn subdivided(m: &SourceMesh) -> SourceMesh {
    let mut out = m.clone();
    out.indices = Vec::with_capacity(m.indices.len() * 4);
    let mut mids: HashMap<([i32; 3], [i32; 3]), u32> = HashMap::new();
    let mut mid = |out: &mut SourceMesh, a: usize, b: usize| -> u32 {
        let (ka, kb) = (key(out.positions[a]), key(out.positions[b]));
        let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
        *mids.entry(k).or_insert_with(|| {
            out.positions.push(scale(add(out.positions[a], out.positions[b]), 0.5));
            out.normals.push(normalize(add(out.normals[a], out.normals[b]), out.normals[a]));
            out.regions.push(out.regions[a]);
            (out.positions.len() - 1) as u32
        })
    };
    for t in m.indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|v| v as usize);
        let (ab, bc, ca) = (mid(&mut out, a, b), mid(&mut out, b, c), mid(&mut out, c, a));
        let (a, b, c) = (a as u32, b as u32, c as u32);
        out.indices.extend([a, ab, ca, ab, b, bc, ca, bc, c, ab, bc, ca]);
    }
    out
}

/// Clay lumps: two octaves of smooth noise (about -1.5 to 1.5).
pub fn lump(p: V3) -> f32 {
    value_noise(scale(p, 2.2)) + 0.5 * value_noise(add(scale(p, 4.7), [5.3, 1.7, 8.9]))
}

/// Knit rows: rounded ridges round the shape, gently wavering (0 to 1).
pub fn ridge(p: V3) -> f32 {
    (p[1] * 18.0 + 1.5 * value_noise(scale(p, 3.0))).sin().abs()
}

/// A repeatable pseudo-random value in -1..=1 for one lattice point.
fn hash(x: i32, y: i32, z: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32).wrapping_mul(0xD816_3841) ^ (z as u32).wrapping_mul(0xCB1A_B31F);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^= h >> 16;
    (h >> 8) as f32 / 16_777_215.0 * 2.0 - 1.0
}

/// Smooth value noise (quintic fade between lattice values).
fn value_noise(p: V3) -> f32 {
    let i = p.map(f32::floor);
    let f = [p[0] - i[0], p[1] - i[1], p[2] - i[2]];
    let w = f.map(|t| t * t * t * (t * (t * 6.0 - 15.0) + 10.0));
    let (x, y, z) = (i[0] as i32, i[1] as i32, i[2] as i32);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let mut layer = [0.0f32; 2];
    for (dz, l) in layer.iter_mut().enumerate() {
        let zz = z + dz as i32;
        let near = lerp(hash(x, y, zz), hash(x + 1, y, zz), w[0]);
        let far = lerp(hash(x, y + 1, zz), hash(x + 1, y + 1, zz), w[0]);
        *l = lerp(near, far, w[1]);
    }
    lerp(layer[0], layer[1], w[2])
}
