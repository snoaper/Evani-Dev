//! Fur (feature `fur`): the material contract, the clump bake, fins and the
//! shell instance layout.
//!
//! Contract (from the surface-bake worker's models): a fur material carries
//! glTF material `extras`
//! `"evani_fur": {"length_m": f, "density": f, "shells": u, "clump_scale_m": f, "tip": [r, g, b]}`
//! with the tip colour in sRGB 0-255. `density` comes in two forms:
//! above 1 it is strands per square centimetre (the strand grid's cells are
//! 1 / sqrt(density) cm wide); at 1 or below it is the coverage share that
//! evani-model-bake writes (its recipes' `fur.density`, 0-1), turned into
//! strands per square centimetre by [`strands_per_cm2`]. `FurSurface::density`
//! is always strands per square centimetre.

use crate::FurSurface;
use serde_json::Value;

/// Most shells a material may ask for.
pub const MAX_SHELLS: u32 = 16;

/// sRGB 0-255 to linear 0-1.
pub fn srgb_to_linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// Strands per square centimetre for a `density` value: above 1 it already
/// is; at 1 or below it is a coverage share, and the strand grid gets about
/// four strand cells across each clump (`clump_scale_m`), thinned by the
/// share: `share * (4 / clump_cm)^2`. A 1 cm clump at share 0.85 gives
/// 13.6 strands per cm^2 (cells about 2.7 mm wide).
pub fn strands_per_cm2(density: f64, clump_scale_m: f64) -> f64 {
    if density > 1.0 {
        return density;
    }
    let clump_cm = (clump_scale_m * 100.0).max(1e-3);
    density * (4.0 / clump_cm).powi(2)
}

fn num(o: &serde_json::Map<String, Value>, key: &str) -> Result<f64, String> {
    o.get(key).and_then(Value::as_f64).ok_or_else(|| format!("evani_fur: `{key}` missing or not a number"))
}

/// The fur numbers in one material's `extras` (None when it is not fur).
pub fn parse_fur(extras: &Value) -> Result<Option<FurSurface>, String> {
    let Some(f) = extras.get("evani_fur") else {
        return Ok(None);
    };
    let o = f.as_object().ok_or("evani_fur: not an object")?;
    let length_m = num(o, "length_m")?;
    let density = num(o, "density")?;
    let shells = o.get("shells").and_then(Value::as_u64).ok_or("evani_fur: `shells` missing or not an unsigned integer")?;
    let clump = num(o, "clump_scale_m")?;
    let tip = o.get("tip").and_then(Value::as_array).ok_or("evani_fur: `tip` missing or not an array")?;
    if tip.len() != 3 {
        return Err("evani_fur: `tip` needs three values".into());
    }
    let mut tip_linear = [0.0; 3];
    for (i, t) in tip.iter().enumerate() {
        let c = t.as_f64().ok_or("evani_fur: `tip` values must be numbers")?;
        if !(0.0..=255.0).contains(&c) {
            return Err(format!("evani_fur: tip value {c} outside 0-255"));
        }
        tip_linear[i] = srgb_to_linear(c.round() as u8);
    }
    if !(length_m > 0.0 && length_m <= 0.2) {
        return Err(format!("evani_fur: length_m {length_m} outside (0, 0.2]"));
    }
    if !(density > 0.0 && density <= 10_000.0) {
        return Err(format!("evani_fur: density {density} outside (0, 10000]"));
    }
    if !(clump > 0.0 && clump <= 1.0) {
        return Err(format!("evani_fur: clump_scale_m {clump} outside (0, 1]"));
    }
    if shells == 0 || shells > MAX_SHELLS as u64 {
        return Err(format!("evani_fur: shells {shells} outside 1-{MAX_SHELLS}"));
    }
    let strands = strands_per_cm2(density, clump);
    if !(strands > 0.0 && strands <= 10_000.0) {
        return Err(format!("evani_fur: density {density} gives {strands} strands per cm^2, outside (0, 10000]"));
    }
    Ok(Some(FurSurface {
        length_m: length_m as f32,
        density: strands as f32,
        shells: shells as u32,
        clump_scale_m: clump as f32,
        tip_linear,
    }))
}

/// Fur per material of a glTF document (index = material index).
pub fn materials_fur(gltf: &Value) -> Result<Vec<Option<FurSurface>>, String> {
    let Some(mats) = gltf.get("materials").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    mats.iter()
        .enumerate()
        .map(|(i, m)| match m.get("extras") {
            Some(e) => parse_fur(e).map_err(|err| format!("material {i}: {err}")),
            None => Ok(None),
        })
        .collect()
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).and_then(|s| <[u8; 4]>::try_from(s).ok()).map(u32::from_le_bytes).ok_or_else(|| "glb: truncated".to_string())
}

/// The JSON chunk and the binary chunk (if any) of a GLB file.
pub fn glb_chunks(bytes: &[u8]) -> Result<(Value, &[u8]), String> {
    if bytes.get(0..4) != Some(b"glTF".as_slice()) {
        return Err("glb: not a GLB file".into());
    }
    if u32_at(bytes, 4)? != 2 {
        return Err("glb: only version 2".into());
    }
    let json_len = u32_at(bytes, 12)? as usize;
    if u32_at(bytes, 16)? != 0x4E4F_534A {
        return Err("glb: first chunk is not JSON".into());
    }
    let json = bytes.get(20..20 + json_len).ok_or("glb: truncated JSON chunk")?;
    let value: Value = serde_json::from_slice(json).map_err(|e| format!("glb: JSON: {e}"))?;
    let bin_at = 20 + json_len;
    let bin = if bytes.len() >= bin_at + 8 && u32_at(bytes, bin_at + 4)? == 0x004E_4942 {
        let n = u32_at(bytes, bin_at)? as usize;
        bytes.get(bin_at + 8..bin_at + 8 + n).ok_or("glb: truncated BIN chunk")?
    } else {
        &[]
    };
    Ok((value, bin))
}

/// Deterministic 32-bit mixing (own constants).
fn mix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x7f4a_7c15);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 16;
    h
}

fn hash2(x: u32, y: u32, seed: u32) -> u32 {
    mix32(x.wrapping_mul(0x9e3d_5a27) ^ y.wrapping_mul(0x6c8e_9cf5) ^ seed.wrapping_mul(0x2545_f491))
}

fn unit(h: u32) -> f32 {
    (h & 0xffff) as f32 / 65535.0
}

/// Bake the tiling clump map: `size` x `size` RGBA8. Clumps are cells of a
/// jittered grid (`clumps` per side); each pixel belongs to the nearest
/// clump centre (wrapping, so the map tiles). R = strand height (1 at a
/// clump's centre, 0.55 at its edge, times a per-clump length), G = per-clump
/// shade, B = per-pixel noise, A = 255. Same inputs, same bytes.
pub fn clump_texture(size: u32, clumps: u32, seed: u32) -> Vec<u8> {
    let size = size.max(4);
    let clumps = clumps.clamp(1, size / 2);
    let cell = size as f32 / clumps as f32;
    let centre = |cx: i64, cy: i64| -> ([f32; 2], u32) {
        let wx = cx.rem_euclid(clumps as i64) as u32;
        let wy = cy.rem_euclid(clumps as i64) as u32;
        let h = hash2(wx, wy, seed);
        let jx = 0.15 + 0.7 * unit(h);
        let jy = 0.15 + 0.7 * unit(mix32(h));
        ([(cx as f32 + jx) * cell, (cy as f32 + jy) * cell], h)
    };
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let (gx, gy) = ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
            let mut best = (f32::MAX, 0u32);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (c, h) = centre(gx + dx, gy + dy);
                    let d = ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt();
                    if d < best.0 {
                        best = (d, h);
                    }
                }
            }
            let edge = (best.0 / (cell * 0.75)).clamp(0.0, 1.0);
            let len = 0.8 + 0.2 * unit(mix32(best.1 ^ 0x51ed));
            let height = (1.0 - 0.45 * edge * edge) * len;
            let shade = 0.25 + 0.75 * unit(mix32(best.1 ^ 0xa3c9));
            let noise = unit(hash2(x, y, seed ^ 0x3d1f));
            out.extend_from_slice(&[(height * 255.0).round() as u8, (shade * 255.0).round() as u8, (noise * 255.0).round() as u8, 255]);
        }
    }
    out
}

/// One fin vertex (fur_shells.wgsl `FinIn` locations 0-3; 40 bytes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FinVertex {
    /// Edge point in the part's local units.
    pub pos: [f32; 3],
    /// Unit normal in local space.
    pub normal: [f32; 3],
    /// u along the edge in metres, v 0 at the root, 1 at the tip.
    pub uv: [f32; 2],
    /// The material's texture coordinate at this edge end (from the
    /// un-welded vertex), so fins take the fur's colour from its base map.
    pub tex: [f32; 2],
}

/// Bytes per fin vertex.
pub const FIN_STRIDE: u64 = 40;

impl FinVertex {
    pub fn push_bytes(&self, out: &mut Vec<u8>) {
        for v in self.pos.iter().chain(self.normal.iter()).chain(self.uv.iter()).chain(self.tex.iter()) {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
}

fn key(p: [f32; 3]) -> [i64; 3] {
    // Positions closer than 0.05 mm (in local units of about a metre) weld.
    [(p[0] * 20_000.0).round() as i64, (p[1] * 20_000.0).round() as i64, (p[2] * 20_000.0).round() as i64]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-12 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 1.0, 0.0] }
}

/// Fins for a fur mesh: one quad standing on every distinct edge (welded by
/// position), rooted on the edge and rising along the welded vertex
/// normals; the shader fades each fin unless its edge is on the
/// silhouette. `metres_per_unit` turns local units into metres for u.
/// `uvs` are the material's texture coordinates per vertex (missing ones
/// read 0, 0); each fin end takes the one of the vertex that first named its
/// edge. Edges in deterministic first-seen order. Indices are u32 triangles.
pub fn build_fins(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uvs: &[[f32; 2]],
    indices: &[u32],
    metres_per_unit: f32,
) -> (Vec<FinVertex>, Vec<u32>) {
    use std::collections::{HashMap, HashSet};
    // Weld: position key -> welded id, summing normals.
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    let mut weld: Vec<u32> = Vec::with_capacity(positions.len());
    let mut wpos: Vec<[f32; 3]> = Vec::new();
    let mut wnorm: Vec<[f32; 3]> = Vec::new();
    for (i, p) in positions.iter().enumerate() {
        let n = normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
        let next = wpos.len() as u32;
        let id = *ids.entry(key(*p)).or_insert(next);
        if id == next {
            wpos.push(*p);
            wnorm.push([0.0; 3]);
        }
        let s = &mut wnorm[id as usize];
        s[0] += n[0];
        s[1] += n[1];
        s[2] += n[2];
        weld.push(id);
    }
    let wnorm: Vec<[f32; 3]> = wnorm.into_iter().map(normalize).collect();
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    for &[i0, i1, i2] in indices.as_chunks::<3>().0 {
        for (a, b) in [(i0, i1), (i1, i2), (i2, i0)] {
            let (Some(&wa), Some(&wb)) = (weld.get(a as usize), weld.get(b as usize)) else {
                continue;
            };
            if wa == wb {
                continue;
            }
            let e = (wa.min(wb), wa.max(wb));
            if !seen.insert(e) {
                continue;
            }
            let uv_of = |i: u32| uvs.get(i as usize).copied().unwrap_or([0.0, 0.0]);
            let (ta, tb) = if wa < wb { (uv_of(a), uv_of(b)) } else { (uv_of(b), uv_of(a)) };
            let (pa, pb) = (wpos[e.0 as usize], wpos[e.1 as usize]);
            let (na, nb) = (wnorm[e.0 as usize], wnorm[e.1 as usize]);
            let len = ((pb[0] - pa[0]).powi(2) + (pb[1] - pa[1]).powi(2) + (pb[2] - pa[2]).powi(2)).sqrt() * metres_per_unit;
            let base = verts.len() as u32;
            verts.push(FinVertex { pos: pa, normal: na, uv: [0.0, 0.0], tex: ta });
            verts.push(FinVertex { pos: pb, normal: nb, uv: [len, 0.0], tex: tb });
            verts.push(FinVertex { pos: pa, normal: na, uv: [0.0, 1.0], tex: ta });
            verts.push(FinVertex { pos: pb, normal: nb, uv: [len, 1.0], tex: tb });
            tris.extend_from_slice(&[base, base + 1, base + 3, base, base + 3, base + 2]);
        }
    }
    (verts, tris)
}

/// The shell pass's instance buffer: each instance's words repeated
/// `shells` times in a row, so the shader's shell is
/// `instance_index % shells`. Draw `instances * shells` instances.
pub fn shell_instances(words: &[f32], words_per_instance: usize, shells: u32) -> Vec<f32> {
    if words_per_instance == 0 || shells == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(words.len() * shells as usize);
    for inst in words.chunks_exact(words_per_instance) {
        for _ in 0..shells {
            out.extend_from_slice(inst);
        }
    }
    out
}
