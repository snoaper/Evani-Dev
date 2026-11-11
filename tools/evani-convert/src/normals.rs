//! Normals for triangle meshes that have none (found by the quality gate,
//! S3-6). The engine draws such a mesh with smooth normals summed over its
//! own index buffer (evani-render `model.rs`, `smooth_normals`). gltfpack
//! merges vertices that share a position, which changes that sum, so a
//! converted file shaded differently from its master (kart_pip: SSIMULACRA2
//! -51 at full quality). The converter therefore writes the engine's own
//! normals into its working copy first; the master stays untouched.

use std::path::Path;

use serde_json::{Value, json};

use crate::glb::Doc;

/// The engine's normals for a mesh without them: face normals (area
/// weighted) summed per vertex index, then normalised. Same arithmetic as
/// evani-render's `smooth_normals`, so the converted file shades like the master.
pub fn engine_normals(pos: &[f32], idx: &[u32]) -> Vec<f32> {
    let mut n = vec![0.0f32; pos.len()];
    for t in idx.as_chunks::<3>().0 {
        let p = |i: u32| {
            let i = i as usize * 3;
            [pos.get(i).copied().unwrap_or(0.0), pos.get(i + 1).copied().unwrap_or(0.0), pos.get(i + 2).copied().unwrap_or(0.0)]
        };
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let f = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &i in t {
            for (k, fk) in f.iter().enumerate() {
                if let Some(x) = n.get_mut(i as usize * 3 + k) {
                    *x += fk;
                }
            }
        }
    }
    for c in n.as_chunks_mut::<3>().0 {
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt().max(1e-12);
        c.iter_mut().for_each(|x| *x /= l);
    }
    n
}

/// Write `master` with the engine's normals added to every triangle
/// primitive that has none to `out`. Returns how many primitives gained
/// normals (0: nothing written; compressed inputs are left alone).
pub fn add_engine_normals(master: &Path, out: &Path) -> Result<usize, String> {
    let bytes = std::fs::read(master).map_err(|e| format!("{}: {e}", master.display()))?;
    let mut d = Doc::read_glb(&bytes).map_err(|e| format!("{}: {e}", master.display()))?;
    let compressed = d.arr("extensionsUsed").iter().any(|e| matches!(e.as_str(), Some("EXT_meshopt_compression" | "KHR_draco_mesh_compression")));
    if compressed || d.arr("buffers").len() != 1 {
        return Ok(0);
    }
    let g = evani_gltf::Gltf::from_glb(&bytes).map_err(|e| format!("{}: {}", master.display(), e.0))?;
    let mut added = 0;
    for (mi, m) in g.meshes.iter().enumerate() {
        for (pi, p) in m.primitives.iter().enumerate() {
            if p.mode != 4 || p.attribute("NORMAL").is_some() {
                continue;
            }
            let Some(pa) = p.attribute("POSITION") else { continue };
            let pos = g.read_f32(pa).map_err(|e| format!("{}: mesh {mi} positions: {}", master.display(), e.0))?;
            let count = pos.len() / 3;
            let idx = match p.indices {
                Some(i) => g.read_u32(i).map_err(|e| format!("{}: mesh {mi} indices: {}", master.display(), e.0))?,
                None => (0..count as u32).collect(),
            };
            let nrm = engine_normals(&pos, &idx);
            while !d.bin.len().is_multiple_of(4) {
                d.bin.push(0);
            }
            let offset = d.bin.len();
            for x in &nrm {
                d.bin.extend_from_slice(&x.to_le_bytes());
            }
            let views = d.json.get_mut("bufferViews").and_then(Value::as_array_mut).ok_or("no bufferViews")?;
            views.push(json!({ "buffer": 0, "byteOffset": offset, "byteLength": nrm.len() * 4, "target": 34962 }));
            let view = views.len() - 1;
            let accs = d.json.get_mut("accessors").and_then(Value::as_array_mut).ok_or("no accessors")?;
            accs.push(json!({ "bufferView": view, "componentType": 5126, "count": count, "type": "VEC3" }));
            let acc = accs.len() - 1;
            let attrs = d.json.pointer_mut(&format!("/meshes/{mi}/primitives/{pi}/attributes")).and_then(Value::as_object_mut).ok_or("primitive without attributes")?;
            attrs.insert("NORMAL".into(), Value::from(acc));
            added += 1;
        }
    }
    if added > 0 {
        std::fs::write(out, d.to_glb()?).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    Ok(added)
}

/// Per vertex of one triangle list: how many non-degenerate triangles use
/// it, and how many of those its normal points against (cosine below -0.5,
/// the rule of evani-gltf's `shipped_model_normals_face_their_triangles`).
fn flip_counts(pos: &[f32], nrm: &[f32], idx: &[u32]) -> Vec<(u32, u32)> {
    let count = pos.len() / 3;
    let mut uses = vec![(0u32, 0u32); count];
    for t in idx.as_chunks::<3>().0 {
        let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
        if a.max(b).max(c) >= count {
            continue;
        }
        let (pa, pb, pc) = (at(pos, a), at(pos, b), at(pos, c));
        let f = cross(sub(pb, pa), sub(pc, pa));
        let fl = len(f);
        if fl < 1e-10 {
            continue;
        }
        for k in [a, b, c] {
            let n = at(nrm, k);
            let nl = len(n);
            if nl < 1e-6 {
                continue;
            }
            uses[k].0 += 1;
            if (n[0] * f[0] + n[1] * f[1] + n[2] * f[2]) / (nl * fl) < -0.5 {
                uses[k].1 += 1;
            }
        }
    }
    uses
}

fn at(a: &[f32], i: usize) -> [f32; 3] {
    [a.get(i * 3).copied().unwrap_or(0.0), a.get(i * 3 + 1).copied().unwrap_or(0.0), a.get(i * 3 + 2).copied().unwrap_or(0.0)]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(u: [f32; 3], v: [f32; 3]) -> [f32; 3] {
    [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
}

fn len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// How many vertices of `glb` (every triangle primitive of every mesh) have
/// a normal that points against every triangle using it: a corner shaded as
/// if lit from behind. Rounding the positions can leave one where a sliver
/// turns over (char-head.medium.glb's inner eye corner, material-view round
/// 4; see [`snap_slivers`]). The converter's report fails a tier with any
/// (read on the uncompressed twin), as evani-gltf's
/// `shipped_model_normals_face_their_triangles` would after it.
///
/// Judged in the world, as that test does: each mesh node's positions and
/// normals through the node's transform (a quantized file's scale lives on
/// the node), the face turned round under a mirrored node (glTF's rule).
pub fn flipped_vertices(glb: &[u8]) -> Result<usize, String> {
    use evani_gltf::transform_point;
    let g = evani_gltf::Gltf::from_glb(glb).map_err(|e| e.0)?;
    let doc: Value = serde_json::from_slice(evani_gltf::split_glb(glb).map_err(|e| e.0)?.0).map_err(|e| e.to_string())?;
    let mats = g.world_matrices();
    let mut bad = 0;
    for (ni, node) in doc["nodes"].as_array().into_iter().flatten().enumerate() {
        let (Some(mi), Some(m)) = (node["mesh"].as_u64(), mats.get(ni)) else { continue };
        let lin = |v: [f32; 3]| sub(transform_point(m, v), transform_point(m, [0.0; 3]));
        let [x, y, z] = [lin([1.0, 0.0, 0.0]), lin([0.0, 1.0, 0.0]), lin([0.0, 0.0, 1.0])];
        let det = x[0] * (y[1] * z[2] - y[2] * z[1]) - y[0] * (x[1] * z[2] - x[2] * z[1]) + z[0] * (x[1] * y[2] - x[2] * y[1]);
        let Some(mesh) = g.meshes.get(mi as usize) else { continue };
        for p in &mesh.primitives {
            let (Some(pa), Some(na)) = (p.attribute("POSITION"), p.attribute("NORMAL")) else { continue };
            if p.mode != 4 {
                continue;
            }
            let pos: Vec<f32> = g.read_f32(pa).map_err(|e| e.0)?.as_chunks::<3>().0.iter().flat_map(|q| transform_point(m, *q)).collect();
            let nrm: Vec<f32> = g.read_f32(na).map_err(|e| e.0)?.as_chunks::<3>().0.iter().flat_map(|q| lin(*q)).collect();
            let mut idx = match p.indices {
                Some(i) => g.read_u32(i).map_err(|e| e.0)?,
                None => (0..(pos.len() / 3) as u32).collect(),
            };
            if det < 0.0 {
                // A mirrored node's front face is clockwise.
                idx.as_chunks_mut::<3>().0.iter_mut().for_each(|t| t.swap(1, 2));
            }
            bad += flip_counts(&pos, &nrm, &idx).iter().filter(|u| u.0 > 0 && u.1 == u.0).count();
        }
    }
    Ok(bad)
}

/// Snap, in place, the slivers of a float (`-noq`) document that rounding
/// its positions to `vp` bits could turn over. A triangle whose height over
/// its longest edge is under two rounding steps (gltfpack rounds each
/// coordinate to one of 2^vp - 1 steps over the widest extent) draws
/// nothing, but rounded it can come out facing the other way and leave a
/// corner lit from behind: char-head.medium.glb (material-view round 4) had
/// a 1.4 mm sliver 37 um thick, one step, in the inner eye corner, sound
/// before rounding and turned over after. Its tip (the corner opposite the
/// longest edge) moves onto the nearer end of that edge, so the triangle
/// rounds to exactly flat (two equal corners) and draws and counts as
/// nothing; the tip's other triangles move by less than that short edge.
/// Returns how many tips moved. Quantized or compressed positions are left
/// alone.
pub fn snap_slivers(d: &mut Doc, vp: u32) -> Result<usize, String> {
    let bytes = d.to_glb()?;
    let g = evani_gltf::Gltf::from_glb(&bytes).map_err(|e| e.0)?;
    // A float VEC3 accessor's byte offset of element 0, and its stride.
    let place = |d: &Doc, acc: usize| -> Option<(usize, usize)> {
        let a = d.json.pointer(&format!("/accessors/{acc}"))?;
        if a.get("componentType")?.as_u64()? != 5126 || a.get("sparse").is_some() {
            return None;
        }
        let v = d.json.pointer(&format!("/bufferViews/{}", a.get("bufferView")?.as_u64()?))?;
        if v.get("buffer").and_then(Value::as_u64).unwrap_or(0) != 0 {
            return None;
        }
        let off = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) + a.get("byteOffset").and_then(Value::as_u64).unwrap_or(0);
        Some((off as usize, v.get("byteStride").and_then(Value::as_u64).map_or(12, |s| s as usize)))
    };
    // The rounding step: the widest extent of every position over 2^vp - 1.
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut prims = Vec::new();
    for m in &g.meshes {
        for p in &m.primitives {
            let Some(pa) = p.attribute("POSITION") else { continue };
            let pos = g.read_f32(pa).map_err(|e| e.0)?;
            for q in pos.as_chunks::<3>().0 {
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
            if p.mode == 4
                && let Some(w) = place(d, pa)
            {
                let idx = match p.indices {
                    Some(i) => g.read_u32(i).map_err(|e| e.0)?,
                    None => (0..(pos.len() / 3) as u32).collect(),
                };
                prims.push((pos, idx, w));
            }
        }
    }
    let extent = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
    if prims.is_empty() || extent <= 0.0 || !(1..=24).contains(&vp) {
        return Ok(0);
    }
    let thin = 2.0 * extent / ((1u32 << vp) - 1) as f32;
    let mut moved = 0;
    for (mut pos, idx, (off, stride)) in prims {
        let tris = idx.as_chunks::<3>().0;
        if tris.iter().flatten().any(|&i| i as usize * 3 + 2 >= pos.len()) {
            continue;
        }
        // The triangles each vertex is in.
        let mut of = vec![Vec::new(); pos.len() / 3];
        for (ti, t) in tris.iter().enumerate() {
            t.iter().for_each(|&i| of[i as usize].push(ti));
        }
        let face = |pos: &[f32], t: &[u32; 3]| {
            let c = t.map(|i| at(pos, i as usize));
            cross(sub(c[1], c[0]), sub(c[2], c[0]))
        };
        // Moving vertex `i` to `to` must not turn any of its other triangles
        // over (or flatten one that has an area): a tip moved a short edge
        // can fold a small neighbour (it did beside the left eye).
        let safe = |pos: &[f32], i: usize, to: [f32; 3], sliver: usize| {
            let mut moved = pos.to_vec();
            moved[i * 3..i * 3 + 3].copy_from_slice(&to);
            of[i].iter().filter(|&&ti| ti != sliver).all(|&ti| {
                let (a, b) = (face(pos, &tris[ti]), face(&moved, &tris[ti]));
                let la = len(a);
                la < 1e-12 || (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) > 0.25 * la * la
            })
        };
        // A moved tip changes its other triangles: a few passes settle it.
        for _ in 0..4 {
            let mut any = false;
            for (ti, t) in tris.iter().enumerate() {
                let v = t.map(|i| i as usize);
                let c = v.map(|i| at(&pos, i));
                if c[0] == c[1] || c[1] == c[2] || c[2] == c[0] {
                    continue;
                }
                // Edge k runs from corner k to corner k + 1; its tip is k + 2.
                let e = [0, 1, 2].map(|k| len(sub(c[(k + 1) % 3], c[k])));
                let k = (0..3).fold(0, |b, k| if e[k] > e[b] { k } else { b });
                if len(cross(sub(c[1], c[0]), sub(c[2], c[0]))) / e[k] >= thin {
                    continue;
                }
                let tip = (k + 2) % 3;
                let i = v[tip];
                let (near, far) = if len(sub(c[tip], c[k])) <= len(sub(c[tip], c[(k + 1) % 3])) { (c[k], c[(k + 1) % 3]) } else { (c[(k + 1) % 3], c[k]) };
                let Some(to) = [near, far].into_iter().find(|&to| safe(&pos, i, to, ti)) else { continue };
                pos[i * 3..i * 3 + 3].copy_from_slice(&to);
                let w = off + i * stride;
                for (j, x) in to.iter().enumerate() {
                    if let Some(s) = d.bin.get_mut(w + j * 4..w + j * 4 + 4) {
                        s.copy_from_slice(&x.to_le_bytes());
                    }
                }
                moved += 1;
                any = true;
            }
            if !any {
                break;
            }
        }
    }
    Ok(moved)
}

/// Smooth normals over one index buffer, as the geometry check builds them
/// for a mesh it validates (tools/evani-geocheck `mesh_of`): area-weighted
/// face normals summed per vertex index, then made unit length.
fn index_normals(pos: &[f32], idx: &[u32]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32; 3]; pos.len() / 3];
    for t in idx.as_chunks::<3>().0 {
        let c = t.map(|i| at(pos, i as usize));
        let f = cross(sub(c[1], c[0]), sub(c[2], c[0]));
        for &i in t {
            if let Some(v) = n.get_mut(i as usize) {
                *v = [v[0] + f[0], v[1] + f[1], v[2] + f[2]];
            }
        }
    }
    for v in &mut n {
        let l = len(*v).max(1e-12);
        *v = [v[0] / l, v[1] / l, v[2] / l];
    }
    n
}

/// Triangles folded over against their surroundings: the face points
/// against the sum of its corners' smooth normals ([`index_normals`]), the
/// geometry check's `inverted` test (meshcheck.rs), zero-area ones skipped.
pub fn folded_triangles(pos: &[f32], idx: &[u32]) -> Vec<usize> {
    let n = index_normals(pos, idx);
    let mut out = Vec::new();
    for (ti, t) in idx.as_chunks::<3>().0.iter().enumerate() {
        let v = t.map(|i| i as usize);
        if v.iter().any(|&i| i >= n.len()) {
            continue;
        }
        let c = v.map(|i| at(pos, i));
        let f = cross(sub(c[1], c[0]), sub(c[2], c[0]));
        let s = v.iter().fold([0.0f32; 3], |a, &i| [a[0] + n[i][0], a[1] + n[i][1], a[2] + n[i][2]]);
        if len(f) >= 1e-12 && len(s) > 1e-9 && f[0] * s[0] + f[1] * s[1] + f[2] * s[2] < 0.0 {
            out.push(ti);
        }
    }
    out
}

/// Unfold, in place, the stray triangles simplifying folds over
/// ([`folded_triangles`]): a coarse triangle across a crease or a thin part
/// can come out facing against its neighbours (char-head.glb's brow tail,
/// lid corner and jaw crease, material-view round 5). Each corner of a
/// folded triangle moves half way to the middle of its neighbours, every
/// copy of a split vertex (a UV or normal seam) with it so no seam opens,
/// a few rounds until none is folded; the round with the fewest folds is
/// kept. Folds are judged on the positions as they will be rounded to `vp`
/// bits (the grid of [`snap_slivers`]: the widest extent over 2^vp - 1
/// steps from the lowest corner), so rounding cannot fold a tiny triangle
/// back afterwards. Only stray folds are touched: a primitive with more than
/// one triangle in twenty folded is wound that way on purpose and stays as
/// authored, and double-sided materials are skipped (their winding draws
/// nothing; the look test's inside-out sphere is one). Moves stay inside the
/// neighbours' hull, so the bounds hold. Float positions only (the merged
/// `-noq` levels); returns how many vertices moved.
pub fn unfold_triangles(d: &mut Doc, vp: u32) -> Result<usize, String> {
    let bytes = d.to_glb()?;
    let g = evani_gltf::Gltf::from_glb(&bytes).map_err(|e| e.0)?;
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for m in &g.meshes {
        for p in &m.primitives {
            let Some(pa) = p.attribute("POSITION") else { continue };
            for q in g.read_f32(pa).map_err(|e| e.0)?.as_chunks::<3>().0 {
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
        }
    }
    let extent = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
    let step = if extent > 0.0 && (1..=24).contains(&vp) { extent / ((1u32 << vp) - 1) as f32 } else { 0.0 };
    let rounded = |pos: &[f32]| -> Vec<f32> {
        if step <= 0.0 {
            return pos.to_vec();
        }
        pos.iter().enumerate().map(|(i, &x)| lo[i % 3] + ((x - lo[i % 3]) / step).round() * step).collect()
    };
    let place = |d: &Doc, acc: usize| -> Option<(usize, usize)> {
        let a = d.json.pointer(&format!("/accessors/{acc}"))?;
        if a.get("componentType")?.as_u64()? != 5126 || a.get("sparse").is_some() {
            return None;
        }
        let v = d.json.pointer(&format!("/bufferViews/{}", a.get("bufferView")?.as_u64()?))?;
        if v.get("buffer").and_then(Value::as_u64).unwrap_or(0) != 0 {
            return None;
        }
        let off = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) + a.get("byteOffset").and_then(Value::as_u64).unwrap_or(0);
        Some((off as usize, v.get("byteStride").and_then(Value::as_u64).map_or(12, |s| s as usize)))
    };
    let double_sided = |mat: Option<usize>| mat.is_some_and(|m| d.json.pointer(&format!("/materials/{m}/doubleSided")).and_then(Value::as_bool) == Some(true));
    let folds = |pos: &[f32], idx: &[u32]| folded_triangles(&rounded(pos), idx);
    let mut moved = 0;
    for m in &g.meshes {
        for p in &m.primitives {
            let Some(pa) = p.attribute("POSITION") else { continue };
            if p.mode != 4 || double_sided(p.material) {
                continue;
            }
            let Some((off, stride)) = place(d, pa) else { continue };
            let start = g.read_f32(pa).map_err(|e| e.0)?;
            let idx = match p.indices {
                Some(i) => g.read_u32(i).map_err(|e| e.0)?,
                None => (0..(start.len() / 3) as u32).collect(),
            };
            if idx.iter().any(|&i| i as usize * 3 + 2 >= start.len()) {
                continue;
            }
            let tris = idx.as_chunks::<3>().0;
            let first = folds(&start, &idx).len();
            if first == 0 || first * 20 > tris.len() {
                continue;
            }
            // Vertices at one position (bit for bit) move together.
            let count = start.len() / 3;
            let mut group_of = vec![0usize; count];
            let mut groups: Vec<Vec<usize>> = Vec::new();
            let mut seen: std::collections::HashMap<[u32; 3], usize> = std::collections::HashMap::new();
            for (i, slot) in group_of.iter_mut().enumerate() {
                let key = at(&start, i).map(f32::to_bits);
                let gi = *seen.entry(key).or_insert_with(|| {
                    groups.push(Vec::new());
                    groups.len() - 1
                });
                groups[gi].push(i);
                *slot = gi;
            }
            let mut ring: Vec<Vec<usize>> = vec![Vec::new(); groups.len()];
            for t in tris {
                let gs = t.map(|i| group_of[i as usize]);
                for k in 0..3 {
                    for j in [(k + 1) % 3, (k + 2) % 3] {
                        if !ring[gs[k]].contains(&gs[j]) {
                            ring[gs[k]].push(gs[j]);
                        }
                    }
                }
            }
            let mut pos = start.clone();
            let (mut best, mut best_pos) = (first, start.clone());
            for _ in 0..8 {
                let bad = folds(&pos, &idx);
                if bad.len() < best {
                    best = bad.len();
                    best_pos.clone_from(&pos);
                }
                if bad.is_empty() {
                    break;
                }
                let mut targets: Vec<usize> = bad.iter().flat_map(|&ti| tris[ti].map(|i| group_of[i as usize])).collect();
                targets.sort_unstable();
                targets.dedup();
                let now = pos.clone();
                for gi in targets {
                    let r = &ring[gi];
                    if r.is_empty() {
                        continue;
                    }
                    let mid = r.iter().fold([0.0f32; 3], |a, &h| {
                        let q = at(&now, groups[h][0]);
                        [a[0] + q[0], a[1] + q[1], a[2] + q[2]]
                    });
                    let mid = mid.map(|x| x / r.len() as f32);
                    let here = at(&now, groups[gi][0]);
                    let to = [0, 1, 2].map(|k| 0.5 * (here[k] + mid[k]));
                    for &i in &groups[gi] {
                        pos[i * 3..i * 3 + 3].copy_from_slice(&to);
                    }
                }
            }
            if folds(&pos, &idx).len() < best {
                best_pos = pos;
            }
            for i in 0..count {
                if best_pos[i * 3..i * 3 + 3] == start[i * 3..i * 3 + 3] {
                    continue;
                }
                moved += 1;
                let w = off + i * stride;
                for j in 0..3 {
                    if let Some(s) = d.bin.get_mut(w + j * 4..w + j * 4 + 4) {
                        s.copy_from_slice(&best_pos[i * 3 + j].to_le_bytes());
                    }
                }
            }
        }
    }
    Ok(moved)
}

/// Repair, in place, the vertex normals of a float (`-noq`) document that
/// point against every triangle using them (the corners [`flipped_vertices`]
/// counts, the rule of evani-gltf's normals test). Simplifying and unfolding
/// move a corner without turning its inherited normal, so a coarse level can
/// keep one lit from behind: char-head.low.glb's far level had one at the
/// lips after the master was clean (material-view round 5). Each such normal
/// becomes the area-weighted normal of its triangles, judged on the positions
/// as the tier's `vp` bits will round them. A primitive with more than one
/// vertex in twenty flagged is wound that way on purpose (a mirrored or inside
/// out part) and stays as authored. Float NORMAL only; returns how many
/// normals were replaced.
pub fn repair_flipped_normals(d: &mut Doc, vp: u32) -> Result<usize, String> {
    let bytes = d.to_glb()?;
    let g = evani_gltf::Gltf::from_glb(&bytes).map_err(|e| e.0)?;
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for m in &g.meshes {
        for p in &m.primitives {
            let Some(pa) = p.attribute("POSITION") else { continue };
            for q in g.read_f32(pa).map_err(|e| e.0)?.as_chunks::<3>().0 {
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
            }
        }
    }
    let extent = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
    let step = if extent > 0.0 && (1..=24).contains(&vp) { extent / ((1u32 << vp) - 1) as f32 } else { 0.0 };
    let rounded = |pos: &[f32]| -> Vec<f32> {
        if step <= 0.0 {
            return pos.to_vec();
        }
        pos.iter().enumerate().map(|(i, &x)| lo[i % 3] + ((x - lo[i % 3]) / step).round() * step).collect()
    };
    let place = |d: &Doc, acc: usize| -> Option<(usize, usize)> {
        let a = d.json.pointer(&format!("/accessors/{acc}"))?;
        if a.get("componentType")?.as_u64()? != 5126 || a.get("sparse").is_some() || a.get("normalized").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        let v = d.json.pointer(&format!("/bufferViews/{}", a.get("bufferView")?.as_u64()?))?;
        if v.get("buffer").and_then(Value::as_u64).unwrap_or(0) != 0 {
            return None;
        }
        let off = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) + a.get("byteOffset").and_then(Value::as_u64).unwrap_or(0);
        Some((off as usize, v.get("byteStride").and_then(Value::as_u64).map_or(12, |s| s as usize)))
    };
    let mut fixed = 0;
    for m in &g.meshes {
        for p in &m.primitives {
            let (Some(pa), Some(na)) = (p.attribute("POSITION"), p.attribute("NORMAL")) else { continue };
            if p.mode != 4 {
                continue;
            }
            let Some((off, stride)) = place(d, na) else { continue };
            let pos = rounded(&g.read_f32(pa).map_err(|e| e.0)?);
            let nrm = g.read_f32(na).map_err(|e| e.0)?;
            let idx = match p.indices {
                Some(i) => g.read_u32(i).map_err(|e| e.0)?,
                None => (0..(pos.len() / 3) as u32).collect(),
            };
            if idx.iter().any(|&i| i as usize * 3 + 2 >= pos.len() || i as usize * 3 + 2 >= nrm.len()) {
                continue;
            }
            let uses = flip_counts(&pos, &nrm, &idx);
            let bad: Vec<usize> = (0..uses.len()).filter(|&i| uses[i].0 > 0 && uses[i].1 == uses[i].0).collect();
            if bad.is_empty() || bad.len() * 20 > uses.iter().filter(|u| u.0 > 0).count() {
                continue;
            }
            // The engine's own smooth normal over this vertex's triangles.
            let faces = index_normals(&pos, &idx);
            for i in bad {
                let n = faces[i];
                if len(n) < 0.5 {
                    continue;
                }
                let w = off + i * stride;
                for (j, x) in n.iter().enumerate() {
                    if let Some(s) = d.bin.get_mut(w + j * 4..w + j * 4 + 4) {
                        s.copy_from_slice(&x.to_le_bytes());
                    }
                }
                fixed += 1;
            }
        }
    }
    Ok(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_normals_are_unit_and_face_the_winding() {
        // One triangle in the xy plane, counter-clockwise: +z.
        let n = engine_normals(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &[0, 1, 2]);
        for c in n.as_chunks::<3>().0 {
            assert!((c[2] - 1.0).abs() < 1e-6 && c[0].abs() < 1e-6 && c[1].abs() < 1e-6, "{c:?}");
        }
    }

    /// Two triangles (0, 1, 2) and (0, 2, 3) over four float corners, corner
    /// 3 at `c3`, with z normals `nz` per corner.
    fn quad(nz: [f32; 4], c3: [f32; 3]) -> Doc {
        let mut bin = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], c3] {
            p.iter().for_each(|c| bin.extend_from_slice(&c.to_le_bytes()));
        }
        for z in nz {
            [0.0f32, 0.0, z].iter().for_each(|c| bin.extend_from_slice(&c.to_le_bytes()));
        }
        for i in [0u32, 1, 2, 0, 2, 3] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let json = json!({
            "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2}]}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0]},
                {"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3"},
                {"bufferView": 2, "componentType": 5125, "count": 6, "type": "SCALAR"}
            ],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 48},
                {"buffer": 0, "byteOffset": 48, "byteLength": 48},
                {"buffer": 0, "byteOffset": 96, "byteLength": 24}
            ],
            "buffers": [{"byteLength": 120}]
        });
        Doc { json, bin }
    }

    /// A flat fan of 22 triangles round a centre in the xy plane, wound +z,
    /// with +z normals except the rim corners in `away`, which point -z
    /// (what a coarse simplified triangle can inherit across a crease).
    fn fan(away: &[usize]) -> Doc {
        let n = 22usize;
        let mut bin = Vec::new();
        let mut pos = vec![[0.0f32; 3]];
        for k in 0..n {
            let a = std::f32::consts::TAU * k as f32 / n as f32;
            pos.push([a.cos(), a.sin(), 0.0]);
        }
        for p in &pos {
            p.iter().for_each(|c| bin.extend_from_slice(&c.to_le_bytes()));
        }
        for i in 0..pos.len() {
            let z = if away.contains(&i) { -1.0f32 } else { 1.0 };
            [0.0f32, 0.0, z].iter().for_each(|c| bin.extend_from_slice(&c.to_le_bytes()));
        }
        let mut idx = Vec::new();
        for k in 0..n {
            idx.extend([0u32, 1 + k as u32, 1 + ((k + 1) % n) as u32]);
        }
        idx.iter().for_each(|i| bin.extend_from_slice(&i.to_le_bytes()));
        let v = pos.len();
        let json = json!({
            "asset": {"version": "2.0"}, "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2}]}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": v, "type": "VEC3", "min": [-1, -1, 0], "max": [1, 1, 0]},
                {"bufferView": 1, "componentType": 5126, "count": v, "type": "VEC3"},
                {"bufferView": 2, "componentType": 5125, "count": idx.len(), "type": "SCALAR"}
            ],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": v * 12},
                {"buffer": 0, "byteOffset": v * 12, "byteLength": v * 12},
                {"buffer": 0, "byteOffset": v * 24, "byteLength": idx.len() * 4}
            ],
            "buffers": [{"byteLength": bin.len()}]
        });
        Doc { json, bin }
    }

    /// The guard: a corner whose normal points against both its triangles
    /// is counted, matching normals are not, and a mirrored node turns the
    /// face round with it (glTF's rule).
    #[test]
    fn a_flipped_corner_is_found() {
        let n = |d: Doc| flipped_vertices(&d.to_glb().unwrap()).unwrap();
        let sq = [0.0, 1.0, 0.0];
        assert_eq!(n(quad([1.0; 4], sq)), 0);
        assert_eq!(n(quad([1.0, 1.0, -1.0, 1.0], sq)), 1, "corner 2 points against both its triangles");
        let mirrored = |mut d: Doc| {
            d.json["nodes"][0]["scale"] = json!([-1.0, 1.0, 1.0]);
            n(d)
        };
        assert_eq!(mirrored(quad([1.0; 4], sq)), 0);
        assert_eq!(mirrored(quad([1.0, 1.0, -1.0, 1.0], sq)), 1);
    }

    /// A normal a coarse level inherited pointing against all its triangles
    /// is replaced by the triangles' own; a sound fan and an inside-out one
    /// (every rim corner flipped: wound that way on purpose) stay byte for byte.
    #[test]
    fn a_flipped_normal_is_repaired_and_only_a_stray_one() {
        let n = |d: &Doc| flipped_vertices(&d.to_glb().unwrap()).unwrap();
        let mut d = fan(&[5]);
        assert_eq!(n(&d), 1);
        assert_eq!(repair_flipped_normals(&mut d, 16).unwrap(), 1);
        assert_eq!(n(&d), 0);
        let mut sound = fan(&[]);
        let before = sound.bin.clone();
        assert_eq!(repair_flipped_normals(&mut sound, 16).unwrap(), 0);
        assert_eq!(sound.bin, before);
        let all: Vec<usize> = (1..=22).collect();
        let mut inside_out = fan(&all);
        let before = inside_out.bin.clone();
        assert_eq!(repair_flipped_normals(&mut inside_out, 16).unwrap(), 0);
        assert_eq!(inside_out.bin, before);
    }

    /// The geometry check's `inverted` rule after simplifying: in a flat
    /// fan, rim corner 9 pushed out past corner 10 folds triangle 8 over
    /// against its neighbours today; after the pass no triangle is folded,
    /// only the fold's corners moved, and a sound fan, an all-folded
    /// (inside-out) fan and a fan without folds are left byte for byte.
    #[test]
    fn simplified_triangles_folded_over_are_unfolded() {
        let folded = |d: &Doc| {
            let g = evani_gltf::Gltf::from_glb(&d.to_glb().unwrap()).unwrap();
            folded_triangles(&g.read_f32(0).unwrap(), &g.read_u32(2).unwrap())
        };
        let mut d = fan(&[]);
        // Corner 9 sits on the rim at angle 8/22 turn; put it just beyond
        // corner 10 (9/22).
        let a = std::f32::consts::TAU * 9.15 / 22.0;
        let at9 = 9 * 12;
        d.bin[at9..at9 + 4].copy_from_slice(&a.cos().to_le_bytes());
        d.bin[at9 + 4..at9 + 8].copy_from_slice(&a.sin().to_le_bytes());
        assert!(!folded(&d).is_empty(), "the pushed corner folds a triangle over today");
        let mut fixed = d.clone();
        assert!(unfold_triangles(&mut fixed, 16).unwrap() > 0);
        assert!(folded(&fixed).is_empty(), "{:?}", folded(&fixed));
        let g0 = evani_gltf::Gltf::from_glb(&d.to_glb().unwrap()).unwrap();
        let g1 = evani_gltf::Gltf::from_glb(&fixed.to_glb().unwrap()).unwrap();
        let (p0, p1) = (g0.read_f32(0).unwrap(), g1.read_f32(0).unwrap());
        assert_eq!(&p0[13 * 3..], &p1[13 * 3..], "corners away from the fold stay");
        assert_eq!(g0.read_f32(1).unwrap(), g1.read_f32(1).unwrap(), "normals are untouched");
        let sound = fan(&[]);
        let mut s2 = sound.clone();
        assert_eq!(unfold_triangles(&mut s2, 16).unwrap(), 0);
        assert_eq!(s2.bin, sound.bin);
        // Wound the other way everywhere (inside out on purpose, as the look
        // test's double-sided sphere is): consistent, so nothing is folded.
        let mut inside_out = fan(&[]);
        let n = 22usize;
        let ib = (n + 1) * 24;
        for k in 0..n {
            let o = ib + k * 12;
            let (x, y) = (o + 4, o + 8);
            let a: [u8; 4] = inside_out.bin[x..x + 4].try_into().unwrap();
            let b: [u8; 4] = inside_out.bin[y..y + 4].try_into().unwrap();
            inside_out.bin[x..x + 4].copy_from_slice(&b);
            inside_out.bin[y..y + 4].copy_from_slice(&a);
        }
        let mut i2 = inside_out.clone();
        assert_eq!(unfold_triangles(&mut i2, 16).unwrap(), 0);
        assert_eq!(i2.bin, inside_out.bin);
        // A double-sided material's winding draws nothing: its fold stays.
        let mut two = d.clone();
        two.json["materials"] = json!([{"doubleSided": true}]);
        two.json["meshes"][0]["primitives"][0]["material"] = json!(0);
        let mut t2 = two.clone();
        assert_eq!(unfold_triangles(&mut t2, 16).unwrap(), 0);
        assert_eq!(t2.bin, two.bin);
    }

    /// The fix: a sliver thinner than two rounding steps has its tip moved
    /// onto the nearer end of its long edge (exactly flat, so rounding cannot
    /// turn it over); the same sliver with more bits, and a sound quad, are
    /// left byte for byte.
    #[test]
    fn slivers_under_two_rounding_steps_are_snapped_flat() {
        // Triangle (0, 2, 3): corner 3 lies 1e-4 off the diagonal from
        // (0, 0, 0) to (1, 1, 0), nearer (0, 0, 0), so it is 7e-5 thick. At
        // 14 bits a step is 6.1e-5 (extent 1): under two steps. At 16 bits
        // (1.5e-5) it is not.
        let c3 = [0.3, 0.3 + 1e-4, 0.0];
        let d = quad([1.0; 4], c3);
        let mut more_bits = d.clone();
        assert_eq!(snap_slivers(&mut more_bits, 16).unwrap(), 0);
        assert_eq!(more_bits.bin, d.bin);
        let mut snapped = d.clone();
        assert_eq!(snap_slivers(&mut snapped, 14).unwrap(), 1);
        let g = evani_gltf::Gltf::from_glb(&snapped.to_glb().unwrap()).unwrap();
        assert_eq!(&g.read_f32(0).unwrap()[9..12], &[0.0, 0.0, 0.0], "the tip sits on the nearer end of the long edge");
        assert_eq!(&snapped.bin[..36], &d.bin[..36], "the other corners are untouched");
        let sound = quad([1.0; 4], [0.0, 1.0, 0.0]);
        let mut s2 = sound.clone();
        assert_eq!(snap_slivers(&mut s2, 8).unwrap(), 0);
        assert_eq!(s2.bin, sound.bin);
    }
}
